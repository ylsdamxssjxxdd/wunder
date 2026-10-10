//! Playwright browser provider.
//!
//! Exposes a dsh-style, upstream-native tool surface (`browser_*`) and maps every
//! call onto the existing Playwright bridge commands. The bridge
//! (`browser_bridge.py`) stays the driving engine; this provider only owns the
//! tool surface, the argument mapping and the content-block shaping.

use crate::schemas::ToolSpec;
use crate::services::browser::provider::{
    scope_from_context, BrowserProvider, ContentBlock, ProviderCallFuture, ProviderOutcome,
};
use crate::services::browser::runtime::browser_service;
use crate::services::tools::ToolContext;
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

/// Stable provider id, selected via `browser.provider`.
pub const PROVIDER_ID: &str = "playwright";

/// Default character budget for textual payloads returned to the model.
const DEFAULT_MAX_CHARS: usize = 20000;

pub const TOOL_NAVIGATE: &str = "browser_navigate";
pub const TOOL_NAVIGATE_BACK: &str = "browser_navigate_back";
pub const TOOL_TABS: &str = "browser_tabs";
pub const TOOL_SNAPSHOT: &str = "browser_snapshot";
pub const TOOL_CLICK: &str = "browser_click";
pub const TOOL_TYPE: &str = "browser_type";
pub const TOOL_HOVER: &str = "browser_hover";
pub const TOOL_SELECT_OPTION: &str = "browser_select_option";
pub const TOOL_DRAG: &str = "browser_drag";
pub const TOOL_PRESS_KEY: &str = "browser_press_key";
pub const TOOL_EVALUATE: &str = "browser_evaluate";
pub const TOOL_WAIT_FOR: &str = "browser_wait_for";
pub const TOOL_SCREENSHOT: &str = "browser_take_screenshot";
pub const TOOL_READ_PAGE: &str = "browser_read_page";
pub const TOOL_BATCH: &str = "browser_batch";
pub const TOOL_CLOSE: &str = "browser_close";
pub const TOOL_STATUS: &str = "browser_status";

/// Native tool names, in a stable order.
pub const TOOL_NAMES: &[&str] = &[
    TOOL_NAVIGATE,
    TOOL_NAVIGATE_BACK,
    TOOL_TABS,
    TOOL_SNAPSHOT,
    TOOL_CLICK,
    TOOL_TYPE,
    TOOL_HOVER,
    TOOL_SELECT_OPTION,
    TOOL_DRAG,
    TOOL_PRESS_KEY,
    TOOL_EVALUATE,
    TOOL_WAIT_FOR,
    TOOL_SCREENSHOT,
    TOOL_READ_PAGE,
    TOOL_BATCH,
    TOOL_CLOSE,
    TOOL_STATUS,
];

/// Playwright provider implementation.
pub struct PlaywrightProvider;

impl PlaywrightProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PlaywrightProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserProvider for PlaywrightProvider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn display_name(&self) -> &'static str {
        "Playwright"
    }

    fn tool_specs(&self) -> Vec<ToolSpec> {
        playwright_tool_specs()
    }

    fn call<'a, 'ctx>(
        &'a self,
        context: &'a ToolContext<'ctx>,
        tool: &'a str,
        args: &'a Value,
    ) -> ProviderCallFuture<'a> {
        Box::pin(async move { dispatch(context, tool, args).await })
    }
}

fn raw_spec(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolSpec {
    ToolSpec {
        name: name.to_string(),
        title: None,
        description: description.to_string(),
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        }),
    }
}

fn playwright_tool_specs() -> Vec<ToolSpec> {
    vec![
        raw_spec(
            TOOL_NAVIGATE,
            "Navigate the current tab to a URL.",
            json!({
                "url": {"type": "string", "description": "Absolute http(s) URL to open."},
                "target_id": {"type": "string", "description": "Tab id; defaults to the active tab."},
                "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 120000},
            }),
            &["url"],
        ),
        raw_spec(
            TOOL_NAVIGATE_BACK,
            "Go back in the current tab's session history.",
            json!({
                "target_id": {"type": "string", "description": "Tab id; defaults to the active tab."},
            }),
            &[],
        ),
        raw_spec(
            TOOL_TABS,
            "List, open, select or close browser tabs.",
            json!({
                "action": {"type": "string", "enum": ["list", "new", "select", "close"]},
                "index": {"type": "integer", "minimum": 0, "description": "Tab index for select/close."},
                "url": {"type": "string", "description": "URL for action=new."},
                "target_id": {"type": "string", "description": "Tab id for select/close."},
            }),
            &["action"],
        ),
        raw_spec(
            TOOL_SNAPSHOT,
            "Capture an accessibility snapshot of the current page with element refs usable by browser_click/browser_type.",
            json!({
                "target_id": {"type": "string"},
                "max_chars": {"type": "integer", "minimum": 1},
            }),
            &[],
        ),
        raw_spec(
            TOOL_CLICK,
            "Click an element identified by a snapshot ref or a CSS selector.",
            json!({
                "ref": {"type": "string", "description": "Element ref from browser_snapshot."},
                "selector": {"type": "string", "description": "CSS selector."},
                "element": {"type": "string", "description": "Human readable element description."},
                "double_click": {"type": "boolean"},
                "target_id": {"type": "string"},
            }),
            &[],
        ),
        raw_spec(
            TOOL_TYPE,
            "Type text into an element identified by a snapshot ref or a CSS selector.",
            json!({
                "ref": {"type": "string"},
                "selector": {"type": "string"},
                "text": {"type": "string"},
                "submit": {"type": "boolean", "description": "Press Enter after typing."},
                "target_id": {"type": "string"},
            }),
            &["text"],
        ),
        raw_spec(
            TOOL_HOVER,
            "Hover the mouse over an element.",
            json!({
                "ref": {"type": "string"},
                "selector": {"type": "string"},
                "target_id": {"type": "string"},
            }),
            &[],
        ),
        raw_spec(
            TOOL_SELECT_OPTION,
            "Select one or more options in a <select> element.",
            json!({
                "ref": {"type": "string"},
                "selector": {"type": "string"},
                "value": {
                    "description": "Option value(s) to select.",
                    "anyOf": [
                        {"type": "string"},
                        {"type": "array", "items": {"type": "string"}}
                    ]
                },
                "target_id": {"type": "string"},
            }),
            &[],
        ),
        raw_spec(
            TOOL_DRAG,
            "Drag one element onto another.",
            json!({
                "from_ref": {"type": "string"},
                "from_selector": {"type": "string"},
                "to_ref": {"type": "string"},
                "to_selector": {"type": "string"},
                "target_id": {"type": "string"},
            }),
            &[],
        ),
        raw_spec(
            TOOL_PRESS_KEY,
            "Press a keyboard key, e.g. Enter, Tab, ArrowDown.",
            json!({
                "key": {"type": "string"},
                "target_id": {"type": "string"},
            }),
            &["key"],
        ),
        raw_spec(
            TOOL_EVALUATE,
            "Evaluate a JavaScript expression in the page context and return its value.",
            json!({
                "expression": {"type": "string"},
                "target_id": {"type": "string"},
            }),
            &["expression"],
        ),
        raw_spec(
            TOOL_WAIT_FOR,
            "Wait for a fixed duration, a load state, or for text to appear.",
            json!({
                "wait_ms": {"type": "integer", "minimum": 0},
                "load_state": {"type": "string", "enum": ["load", "domcontentloaded", "networkidle"]},
                "text": {"type": "string"},
                "target_id": {"type": "string"},
            }),
            &[],
        ),
        raw_spec(
            TOOL_SCREENSHOT,
            "Take a screenshot of the current page and return it as an image.",
            json!({
                "path": {"type": "string", "description": "Optional workspace-relative output path."},
                "full_page": {"type": "boolean"},
                "target_id": {"type": "string"},
            }),
            &[],
        ),
        raw_spec(
            TOOL_READ_PAGE,
            "Read the current page as Markdown or text.",
            json!({
                "target_id": {"type": "string"},
                "max_chars": {"type": "integer", "minimum": 1},
            }),
            &[],
        ),
        raw_spec(
            TOOL_BATCH,
            "Run a batch of interaction steps in order (max 10 steps).",
            json!({
                "steps": {
                    "type": "array",
                    "items": {"type": "object"},
                    "maxItems": 10
                },
                "target_id": {"type": "string"},
            }),
            &["steps"],
        ),
        raw_spec(
            TOOL_CLOSE,
            "Close the browser session for this conversation.",
            json!({}),
            &[],
        ),
        raw_spec(
            TOOL_STATUS,
            "Report browser runtime status and open sessions.",
            json!({}),
            &[],
        ),
    ]
}

async fn dispatch(
    context: &ToolContext<'_>,
    tool: &str,
    args: &Value,
) -> Result<ProviderOutcome> {
    let service = browser_service(context.config);
    let scope = scope_from_context(context, args);
    let target = opt_str(args, "target_id");

    match tool {
        TOOL_STATUS => {
            let data = service.execute(&scope, "status", &json!({})).await?;
            Ok(text_outcome("Browser status:", data))
        }
        TOOL_CLOSE => {
            let data = service.execute(&scope, "stop", &json!({})).await?;
            Ok(text_outcome("Closed the browser session.", data))
        }
        TOOL_NAVIGATE => {
            let url = required_str(args, "url")?;
            let mut call_args = Map::new();
            call_args.insert("url".to_string(), json!(url));
            copy_opt(args, &mut call_args, "timeout_ms");
            copy_opt(args, &mut call_args, "target_id");
            let data = service
                .execute(&scope, "navigate", &Value::Object(call_args))
                .await?;
            Ok(text_outcome(format!("Navigated to {url}."), data))
        }
        TOOL_NAVIGATE_BACK => {
            let request = json!({"kind": "evaluate", "expression": "history.back()"});
            let data = service
                .execute(&scope, "act", &act_args(request, target.as_deref()))
                .await?;
            Ok(text_outcome("Went back in history.", data))
        }
        TOOL_TABS => {
            let action = opt_str(args, "action").unwrap_or_else(|| "list".to_string());
            match action.trim().to_ascii_lowercase().as_str() {
                "list" | "tabs" => {
                    let data = service.execute(&scope, "tabs", &json!({})).await?;
                    Ok(text_outcome("Open tabs:", data))
                }
                "new" | "open" => {
                    let mut call_args = Map::new();
                    copy_opt(args, &mut call_args, "url");
                    copy_opt(args, &mut call_args, "target_id");
                    let data = service
                        .execute(&scope, "open", &Value::Object(call_args))
                        .await?;
                    Ok(text_outcome("Opened a tab.", data))
                }
                "select" | "focus" => {
                    let mut call_args = Map::new();
                    copy_opt(args, &mut call_args, "target_id");
                    copy_opt(args, &mut call_args, "index");
                    let data = service
                        .execute(&scope, "focus", &Value::Object(call_args))
                        .await?;
                    Ok(text_outcome("Selected a tab.", data))
                }
                "close" => {
                    let mut call_args = Map::new();
                    copy_opt(args, &mut call_args, "target_id");
                    copy_opt(args, &mut call_args, "index");
                    let data = service
                        .execute(&scope, "close", &Value::Object(call_args))
                        .await?;
                    Ok(text_outcome("Closed a tab.", data))
                }
                other => Err(anyhow!(
                    "browser_tabs action must be list/new/select/close, got '{other}'"
                )),
            }
        }
        TOOL_SNAPSHOT => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "max_chars");
            let data = service
                .execute(&scope, "snapshot", &Value::Object(call_args))
                .await?;
            let text = primary_text(&data)
                .unwrap_or_else(|| summarize_data(&data, max_chars_arg(args)));
            Ok(ProviderOutcome::text(text))
        }
        TOOL_READ_PAGE => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "max_chars");
            let data = service
                .execute(&scope, "read_page", &Value::Object(call_args))
                .await?;
            let text = primary_text(&data)
                .unwrap_or_else(|| summarize_data(&data, max_chars_arg(args)));
            Ok(ProviderOutcome::text(text))
        }
        TOOL_CLICK => {
            let request = request_with(
                "click",
                args,
                &["ref", "selector", "element", "double_click"],
            );
            act_outcome(service, &scope, request, target.as_deref(), "Clicked the element.").await
        }
        TOOL_TYPE => {
            let request = request_with("type", args, &["ref", "selector", "text", "submit"]);
            act_outcome(service, &scope, request, target.as_deref(), "Typed into the element.").await
        }
        TOOL_HOVER => {
            let request = request_with("hover", args, &["ref", "selector"]);
            act_outcome(service, &scope, request, target.as_deref(), "Hovered the element.").await
        }
        TOOL_SELECT_OPTION => {
            let request = request_with("select", args, &["ref", "selector", "value"]);
            act_outcome(service, &scope, request, target.as_deref(), "Selected the option.").await
        }
        TOOL_DRAG => {
            let request = request_with(
                "drag",
                args,
                &["from_ref", "from_selector", "to_ref", "to_selector"],
            );
            act_outcome(service, &scope, request, target.as_deref(), "Dragged the element.").await
        }
        TOOL_PRESS_KEY => {
            let key = required_str(args, "key")?;
            let request = json!({"kind": "press", "key": key});
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                format!("Pressed key {key}."),
            )
            .await
        }
        TOOL_EVALUATE => {
            let expression = required_str(args, "expression")?;
            let request = json!({"kind": "evaluate", "expression": expression});
            let data = service
                .execute(&scope, "act", &act_args(request, target.as_deref()))
                .await?;
            let text = primary_text(&data)
                .unwrap_or_else(|| summarize_data(&data, DEFAULT_MAX_CHARS));
            Ok(ProviderOutcome::text(text))
        }
        TOOL_WAIT_FOR => {
            let mut request = Map::new();
            request.insert("kind".to_string(), json!("wait"));
            for key in ["wait_ms", "ms", "load_state", "state", "text"] {
                copy_opt(args, &mut request, key);
            }
            let request = Value::Object(request);
            act_outcome(service, &scope, request, target.as_deref(), "Wait finished.").await
        }
        TOOL_BATCH => {
            let steps = args
                .get("steps")
                .cloned()
                .filter(Value::is_array)
                .ok_or_else(|| anyhow!("browser_batch requires an array 'steps'"))?;
            let request = json!({"kind": "batch", "steps": steps});
            act_outcome(service, &scope, request, target.as_deref(), "Batch finished.").await
        }
        TOOL_SCREENSHOT => {
            let mut call_args = Map::new();
            call_args.insert("save_to_workspace".to_string(), json!(true));
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "full_page");
            let mut data = service
                .execute(&scope, "screenshot", &Value::Object(call_args))
                .await?;
            let format = data
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("png")
                .to_string();
            let mime_type = normalize_image_mime(&format);
            let base64 = data
                .get("image_base64")
                .and_then(Value::as_str)
                .map(str::to_string);
            match base64 {
                Some(encoded) => {
                    if let Value::Object(map) = &mut data {
                        map.remove("image_base64");
                    }
                    Ok(ProviderOutcome::blocks(vec![
                        ContentBlock::text(format!("Captured a {format} screenshot.")),
                        ContentBlock::image(encoded, mime_type),
                    ])
                    .with_structured(data))
                }
                None => Ok(ProviderOutcome::text(format!(
                    "Screenshot returned no image payload: {}",
                    summarize_data(&data, 2000)
                ))
                .with_structured(data)),
            }
        }
        other => Err(anyhow!("Unknown browser tool '{other}'")),
    }
}

async fn act_outcome(
    service: &crate::services::browser::runtime::BrowserControlService,
    scope: &crate::services::browser::model::BrowserSessionScope,
    request: Value,
    target_id: Option<&str>,
    label: impl Into<String>,
) -> Result<ProviderOutcome> {
    let data = service
        .execute(scope, "act", &act_args(request, target_id))
        .await?;
    Ok(text_outcome(label, data))
}

fn act_args(request: Value, target_id: Option<&str>) -> Value {
    let mut map = Map::new();
    map.insert("request".to_string(), request);
    if let Some(id) = target_id.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert("target_id".to_string(), json!(id));
    }
    Value::Object(map)
}

fn request_with(kind: &str, args: &Value, keys: &[&str]) -> Value {
    let mut map = Map::new();
    map.insert("kind".to_string(), json!(kind));
    for key in keys {
        copy_opt(args, &mut map, key);
    }
    Value::Object(map)
}

fn copy_opt(from: &Value, to: &mut Map<String, Value>, key: &str) {
    if let Some(value) = from.get(key) {
        if !value.is_null() {
            to.insert(key.to_string(), value.clone());
        }
    }
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn required_str(args: &Value, key: &str) -> Result<String> {
    opt_str(args, key).ok_or_else(|| anyhow!("Missing required parameter '{key}'"))
}

fn max_chars_arg(args: &Value) -> usize {
    args.get("max_chars")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_MAX_CHARS)
}

/// Turn a bridge payload into a text block, preferring a natural text field.
fn text_outcome(label: impl Into<String>, data: Value) -> ProviderOutcome {
    let label = label.into();
    let body = primary_text(&data).unwrap_or_else(|| summarize_data(&data, DEFAULT_MAX_CHARS));
    let text = if body.trim().is_empty() {
        label
    } else {
        format!("{label}\n{body}")
    };
    ProviderOutcome::text(text).with_structured(data)
}

/// Prefer a human-readable field over raw JSON.
fn primary_text(data: &Value) -> Option<String> {
    for key in ["text", "markdown", "content", "snapshot", "result", "value"] {
        if let Some(value) = data.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn summarize_data(data: &Value, max_chars: usize) -> String {
    let raw = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());
    truncate_chars(&raw, max_chars)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_chars).collect();
    out.push_str("\n...[truncated]");
    out
}

/// Map a bridge image format onto a whitelisted MIME type (dsh parity).
fn normalize_image_mime(format: &str) -> String {
    let lower = format.trim().to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "jpg" | "jpeg" => "jpeg",
        "webp" => "webp",
        "gif" => "gif",
        _ => "png",
    };
    format!("image/{normalized}")
}

#[cfg(test)]
mod tests {
    use super::{normalize_image_mime, truncate_chars, PlaywrightProvider, TOOL_NAMES};
    use crate::services::browser::provider::BrowserProvider;
    use serde_json::json;

    #[test]
    fn provider_exposes_native_tool_surface() {
        let provider = PlaywrightProvider::new();
        let specs = provider.tool_specs();
        let names: Vec<String> = specs.iter().map(|spec| spec.name.clone()).collect();
        assert_eq!(names.len(), TOOL_NAMES.len());
        for name in TOOL_NAMES {
            assert!(provider.owns_tool(name), "missing tool {name}");
            assert!(names.iter().any(|item| item == name));
        }
        assert!(!provider.owns_tool("浏览器"));
        assert!(!provider.owns_tool("browser"));
    }

    #[test]
    fn provider_specs_declare_required_arguments() {
        let provider = PlaywrightProvider::new();
        let navigate = provider
            .tool_specs()
            .into_iter()
            .find(|spec| spec.name == "browser_navigate")
            .expect("navigate spec");
        let required = navigate.input_schema["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|item| item == "url"));
        assert_eq!(navigate.input_schema["type"], json!("object"));
    }

    #[test]
    fn image_mime_is_whitelisted() {
        assert_eq!(normalize_image_mime("png"), "image/png");
        assert_eq!(normalize_image_mime("jpg"), "image/jpeg");
        assert_eq!(normalize_image_mime("jpeg"), "image/jpeg");
        assert_eq!(normalize_image_mime("webp"), "image/webp");
        assert_eq!(normalize_image_mime("gif"), "image/gif");
        assert_eq!(normalize_image_mime("bmp"), "image/png");
    }

    #[test]
    fn truncate_marks_truncated_payloads() {
        assert_eq!(truncate_chars("abc", 10), "abc");
        let truncated = truncate_chars("abcdefghij", 3);
        assert!(truncated.starts_with("abc"));
        assert!(truncated.contains("truncated"));
    }
}