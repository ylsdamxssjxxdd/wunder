//! Playwright browser provider.
//!
//! Exposes a compact six-tool model surface and maps every call onto the existing
//! Playwright bridge commands. Legacy `browser_*` member names remain accepted by
//! the dispatcher, but are deliberately omitted from `tool_specs()` so they do
//! not consume model context. The bridge
//! (`browser_bridge.py`) stays the driving engine; this provider only owns the
//! tool surface, the argument mapping and the content-block shaping.

use crate::schemas::ToolSpec;
use crate::services::browser::provider::{
    scope_from_context, BrowserProvider, ContentBlock, ProviderCallFuture, ProviderOutcome,
};
use crate::services::browser::runtime::{browser_service, BrowserCommandFailure};
use crate::services::tools::ToolContext;
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

/// Stable provider id, selected via `browser.provider`.
pub const PROVIDER_ID: &str = "playwright";

/// Default character budget for textual payloads returned to the model.
const DEFAULT_MAX_CHARS: usize = 20000;

pub const TOOL_NAVIGATE: &str = "browser_navigate";
pub const TOOL_TABS: &str = "browser_tabs";
pub const TOOL_OBSERVE: &str = "browser_observe";
pub const TOOL_INTERACT: &str = "browser_interact";
pub const TOOL_WAIT: &str = "browser_wait";
pub const TOOL_BATCH: &str = "browser_batch";

// Migration-only names. They are callable for persisted sessions and old
// clients, but never returned from tool_specs()/browser_tool_names().
pub const TOOL_NAVIGATE_BACK: &str = "browser_navigate_back";
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
pub const TOOL_CLOSE: &str = "browser_close";
pub const TOOL_STATUS: &str = "browser_status";

/// Native tool names, in a stable order.
pub const TOOL_NAMES: &[&str] = &[
    TOOL_NAVIGATE,
    TOOL_TABS,
    TOOL_OBSERVE,
    TOOL_INTERACT,
    TOOL_WAIT,
    TOOL_BATCH,
];

pub const LEGACY_TOOL_NAMES: &[&str] = &[
    TOOL_NAVIGATE_BACK,
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
    TOOL_CLOSE,
    TOOL_STATUS,
];

/// True for migration-only browser member names accepted from persisted calls.
pub fn is_legacy_tool_name(name: &str) -> bool {
    LEGACY_TOOL_NAMES.contains(&name.trim())
}

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
    fn owns_tool(&self, name: &str) -> bool {
        TOOL_NAMES.contains(&name.trim()) || LEGACY_TOOL_NAMES.contains(&name.trim())
    }
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
        Box::pin(async move {
            match dispatch(context, tool, args).await {
                Err(error) => match error.downcast_ref::<BrowserCommandFailure>() {
                    Some(failure) => Ok(ProviderOutcome::failure(error.to_string())
                        .with_structured(failure.data.clone())),
                    None => Err(error),
                },
                result => result,
            }
        })
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
    let mut interact = raw_spec(
        TOOL_INTERACT,
        "Act on the active page. Prefer refs from observe; use select for a native select control. Known dependent actions belong in batch.",
        json!({
            "action": {"type": "string", "enum": ["click", "type", "hover", "select", "drag", "press"]},
            "ref": {"type": "string", "description": "Element ref from browser_observe."},
            "selector": {"type": "string", "description": "CSS selector."},
            "double_click": {"type": "boolean"}, "text": {"type": "string"},
            "submit": {"type": "boolean", "description": "Press Enter after type."},
            "value": {"anyOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]},
            "key": {"type": "string"}, "from_ref": {"type": "string"},
            "from_selector": {"type": "string"}, "to_ref": {"type": "string"},
            "to_selector": {"type": "string"}
        }),
        &["action"],
    );
    interact.input_schema["oneOf"] = json!([
        {"properties":{"action":{"const":"click"}},"anyOf":[{"required":["ref"]},{"required":["selector"]}]},
        {"properties":{"action":{"const":"type"}},"required":["text"],"anyOf":[{"required":["ref"]},{"required":["selector"]}]},
        {"properties":{"action":{"const":"hover"}},"anyOf":[{"required":["ref"]},{"required":["selector"]}]},
        {"properties":{"action":{"const":"select"}},"required":["value"],"anyOf":[{"required":["ref"]},{"required":["selector"]}]},
        {"properties":{"action":{"const":"drag"}},"anyOf":[{"required":["from_ref","to_ref"]},{"required":["from_selector","to_selector"]}]},
        {"properties":{"action":{"const":"press"}},"required":["key"]}
    ]);
    vec![
        raw_spec(
            TOOL_NAVIGATE,
            "Open a URL or go back. Use for interactive pages; then observe refs before interaction.",
            json!({
                "action": {"type": "string", "enum": ["navigate", "back"], "default": "navigate"},
                "url": {"type": "string", "description": "Absolute http(s) URL; required for navigate."},
                "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 120000}
            }),
            &[],
        ),
        raw_spec(
            TOOL_TABS,
            "List, open, select, or close tabs. Later calls use the selected tab.",
            json!({
                "action": {"type": "string", "enum": ["list", "new", "select", "close"]},
                "index": {"type": "integer", "minimum": 0, "description": "Tab index for select/close."},
                "url": {"type": "string", "description": "URL for new."},
                "target_id": {"type": "string", "description": "Id returned by list."}
            }),
            &["action"],
        ),
        raw_spec(
            TOOL_OBSERVE,
            "Read the active page. refs returns actionable element refs; text returns readable content; screenshot returns an attachment.",
            json!({
                "mode": {"type": "string", "enum": ["refs", "text", "screenshot"], "default": "refs"},
                "max_chars": {"type": "integer", "minimum": 1, "maximum": 40000},
                "path": {"type": "string", "description": "Workspace-relative screenshot path."},
                "full_page": {"type": "boolean"}
            }),
            &[],
        ),
        interact,
        raw_spec(
            TOOL_WAIT,
            "Wait for a known selector, text, URL, or load state after an action. A fixed wait alone does not prove success.",
            json!({
                "wait_ms": {"type": "integer", "minimum": 0, "maximum":120000},
                "load_state": {"type": "string", "enum": ["load", "domcontentloaded", "networkidle"]},
                "text": {"type": "string"},
                "selector": {"type": "string"},
                "state": {"type":"string", "enum":["visible","hidden","attached","detached"]},
                "url": {"type": "string", "description": "Expected URL or Playwright glob."},
                "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 120000}
            }),
            &[],
        ),
        raw_spec(
            TOOL_BATCH,
            "Execute 1–10 dependent actions in order. Combine known form input, submission, and a success condition; stops at the failed step.",
            json!({
                "steps": {
                    "type": "array",
                    "items": batch_step_schema(),
                    "minItems": 1,
                    "maxItems": 10
                }
            }),
            &["steps"],
        ),
    ]
}

fn batch_step_schema() -> Value {
    json!({
        "type":"object", "required":["kind"], "additionalProperties":false,
        "properties": {
            "kind":{"type":"string", "enum":["type","click","press","select","hover","drag","wait","navigate"]},
            "ref":{"type":"string"}, "selector":{"type":"string"},
            "text":{"type":"string"}, "submit":{"type":"boolean"},
            "key":{"type":"string"}, "double_click":{"type":"boolean"},
            "value":{"anyOf":[{"type":"string"},{"type":"array","items":{"type":"string"}}]},
            "from_ref":{"type":"string"}, "from_selector":{"type":"string"},
            "to_ref":{"type":"string"}, "to_selector":{"type":"string"},
            "url":{"type":"string"}, "load_state":{"type":"string","enum":["load","domcontentloaded","networkidle"]},
            "state":{"type":"string","enum":["visible","hidden","attached","detached"]},
            "wait_ms":{"type":"integer","minimum":0,"maximum":120000},
            "timeout_ms":{"type":"integer","minimum":1,"maximum":120000}
        }
    })
}

/// Normalize the compact model surface into the legacy action handlers. Keeping
/// this translation at the provider boundary lets old clients and persisted
/// calls continue to use every former browser_* member without exposing them in
/// the model catalog.
async fn dispatch(context: &ToolContext<'_>, tool: &str, args: &Value) -> Result<ProviderOutcome> {
    match tool {
        TOOL_NAVIGATE => {
            if opt_str(args, "action").as_deref() == Some("back") {
                dispatch_legacy(context, TOOL_NAVIGATE_BACK, args).await
            } else {
                // Navigation no longer implicitly observes the page. Observation
                // is an explicit browser_observe call, which prevents a full
                // accessibility snapshot from being appended to every URL hop.
                let mut normalized = args.clone();
                if let Some(map) = normalized.as_object_mut() {
                    map.entry("include_snapshot".to_string())
                        .or_insert(Value::Bool(false));
                }
                dispatch_legacy(context, TOOL_NAVIGATE, &normalized).await
            }
        }
        TOOL_OBSERVE => match opt_str(args, "mode").as_deref().unwrap_or("refs") {
            "text" => dispatch_legacy(context, TOOL_READ_PAGE, args).await,
            "screenshot" => dispatch_legacy(context, TOOL_SCREENSHOT, args).await,
            _ => dispatch_legacy(context, TOOL_SNAPSHOT, args).await,
        },
        TOOL_INTERACT => {
            let action = required_str(args, "action")?;
            let legacy = match action.as_str() {
                "click" => TOOL_CLICK,
                "type" => TOOL_TYPE,
                "hover" => TOOL_HOVER,
                "select" => TOOL_SELECT_OPTION,
                "drag" => TOOL_DRAG,
                "press" => TOOL_PRESS_KEY,
                "evaluate" => TOOL_EVALUATE,
                other => return Err(anyhow!("browser_interact action unsupported: {other}")),
            };
            dispatch_legacy(context, legacy, args).await
        }
        TOOL_WAIT => dispatch_legacy(context, TOOL_WAIT_FOR, args).await,
        _ => dispatch_legacy(context, tool, args).await,
    }
}

async fn dispatch_legacy(
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
            copy_snapshot_args(args, &mut call_args, true);
            let data = service
                .execute(&scope, "navigate", &Value::Object(call_args))
                .await?;
            Ok(text_outcome(format!("Navigated to {url}."), data))
        }
        TOOL_NAVIGATE_BACK => {
            // Fixed history navigation does not grant arbitrary JavaScript.
            let request = json!({"kind": "back"});
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
            copy_opt(args, &mut call_args, "snapshot_timeout_ms");
            let data = service
                .execute(&scope, "snapshot", &Value::Object(call_args))
                .await?;
            let text = data
                .get("snapshot")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(ProviderOutcome::text(text).with_structured(json!({
                "target_id": data.get("target_id"), "url": data.get("url"),
                "truncated": data.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            })))
        }
        TOOL_READ_PAGE => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "max_chars");
            let data = service
                .execute(&scope, "read_page", &Value::Object(call_args))
                .await?;
            let text = data
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(ProviderOutcome::text(text).with_structured(json!({
                "target_id": data.get("target_id"), "url": data.get("url"),
                "truncated": data.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            })))
        }
        TOOL_CLICK => {
            let request = request_with(
                "click",
                args,
                &["ref", "selector", "element", "double_click"],
            );
            provider_act(
                service,
                &scope,
                request,
                target.as_deref(),
                "Clicked the element.",
                args,
            )
            .await
        }
        TOOL_TYPE => {
            let request = request_with("type", args, &["ref", "selector", "text", "submit"]);
            provider_act(
                service,
                &scope,
                request,
                target.as_deref(),
                "Typed into the element.",
                args,
            )
            .await
        }
        TOOL_HOVER => {
            let request = request_with("hover", args, &["ref", "selector"]);
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                "Hovered the element.",
            )
            .await
        }
        TOOL_SELECT_OPTION => {
            let request = request_with("select", args, &["ref", "selector", "value"]);
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                "Selected the option.",
            )
            .await
        }
        TOOL_DRAG => {
            let request = request_with(
                "drag",
                args,
                &["from_ref", "from_selector", "to_ref", "to_selector"],
            );
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                "Dragged the element.",
            )
            .await
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
            let text =
                primary_text(&data).unwrap_or_else(|| summarize_data(&data, DEFAULT_MAX_CHARS));
            // The actual value stays available to projection/result storage.
            Ok(ProviderOutcome::text(text).with_structured(data))
        }
        TOOL_WAIT_FOR => {
            let mut request = Map::new();
            request.insert("kind".to_string(), json!("wait"));
            for key in [
                "wait_ms",
                "load_state",
                "text",
                "selector",
                "state",
                "url",
                "timeout_ms",
            ] {
                copy_opt(args, &mut request, key);
            }
            provider_act(
                service,
                &scope,
                Value::Object(request),
                target.as_deref(),
                "Wait finished.",
                args,
            )
            .await
        }
        TOOL_BATCH => {
            let steps = args
                .get("steps")
                .cloned()
                .filter(Value::is_array)
                .ok_or_else(|| anyhow!("browser_batch requires an array 'steps'"))?;
            let request = json!({"kind": "batch", "steps": steps});
            provider_act(
                service,
                &scope,
                request,
                target.as_deref(),
                "Batch finished.",
                args,
            )
            .await
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
                None => Ok(
                    ProviderOutcome::failure("Screenshot returned no image payload.")
                        .with_structured(data),
                ),
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

async fn provider_act(
    service: &crate::services::browser::runtime::BrowserControlService,
    scope: &crate::services::browser::model::BrowserSessionScope,
    request: Value,
    target_id: Option<&str>,
    label: impl Into<String>,
    args: &Value,
) -> Result<ProviderOutcome> {
    let mut call_args = act_args(request, target_id);
    copy_snapshot_args(args, call_args.as_object_mut().expect("act args"), false);
    let data = service.execute(scope, "act", &call_args).await?;
    Ok(text_outcome(label, data))
}

fn copy_snapshot_args(args: &Value, call_args: &mut Map<String, Value>, default_include: bool) {
    copy_opt(args, call_args, "include_snapshot");
    copy_opt(args, call_args, "max_chars");
    copy_opt(args, call_args, "snapshot_timeout_ms");
    if default_include {
        call_args
            .entry("include_snapshot".to_string())
            .or_insert(json!(true));
    }
    if call_args.get("include_snapshot").and_then(Value::as_bool) == Some(true) {
        call_args
            .entry("max_chars".to_string())
            .or_insert(json!(4000));
    }
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

/// Turn a bridge payload into a text block, preferring a natural text field.
fn text_outcome(label: impl Into<String>, data: Value) -> ProviderOutcome {
    // Structured data already carries the observation; duplicating its JSON in
    // a text block wastes context and used to hide attachment loss.
    ProviderOutcome::text(label).with_structured(data)
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
    input.chars().take(max_chars).collect()
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
    use super::{
        copy_snapshot_args, normalize_image_mime, truncate_chars, PlaywrightProvider,
        LEGACY_TOOL_NAMES, TOOL_NAMES,
    };
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
        for name in LEGACY_TOOL_NAMES {
            assert!(
                provider.owns_tool(name),
                "legacy tool {name} must remain callable"
            );
            assert!(
                !names.iter().any(|item| item == name),
                "legacy tool {name} must not be exposed"
            );
        }
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
        assert!(required.is_empty());
        assert!(navigate.input_schema["properties"]["action"].is_object());
        assert_eq!(navigate.input_schema["type"], json!("object"));
    }

    #[test]
    fn browser_specs_describe_batch_steps_and_observation_options() {
        let specs = PlaywrightProvider::new().tool_specs();
        assert_eq!(specs.len(), 6);
        let batch = specs
            .iter()
            .find(|spec| spec.name == "browser_batch")
            .unwrap();
        let steps = &batch.input_schema["properties"]["steps"];
        assert_eq!(steps["minItems"], 1);
        assert_eq!(steps["maxItems"], 10);
        assert_eq!(steps["items"]["required"], json!(["kind"]));
        assert!(steps["items"]["properties"]["kind"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind == "wait"));
        let interact = specs
            .iter()
            .find(|spec| spec.name == "browser_interact")
            .unwrap();
        assert!(interact.input_schema["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("click")));
        let wait = specs
            .iter()
            .find(|spec| spec.name == "browser_wait")
            .unwrap();
        for key in ["selector", "url", "timeout_ms"] {
            assert!(wait.input_schema["properties"].get(key).is_some());
        }
        assert_eq!(
            wait.input_schema["properties"]["state"]["enum"],
            json!(["visible", "hidden", "attached", "detached"])
        );
    }

    #[test]
    fn legacy_navigation_snapshot_default_can_be_explicitly_disabled() {
        let mut call_args = serde_json::Map::new();
        copy_snapshot_args(&json!({}), &mut call_args, true);
        assert_eq!(call_args["include_snapshot"], true);
        assert_eq!(call_args["max_chars"], 4000);
        let mut call_args = serde_json::Map::new();
        copy_snapshot_args(
            &json!({"include_snapshot":false, "snapshot_timeout_ms":0}),
            &mut call_args,
            true,
        );
        assert_eq!(call_args["include_snapshot"], false);
        assert_eq!(call_args["snapshot_timeout_ms"], 0);
        assert!(!call_args.contains_key("max_chars"));
    }

    #[test]
    fn compact_surface_has_no_implicit_snapshot_parameter() {
        let navigate = PlaywrightProvider::new()
            .tool_specs()
            .into_iter()
            .find(|spec| spec.name == "browser_navigate")
            .unwrap();
        assert!(navigate.input_schema["properties"]["include_snapshot"].is_null());
        assert!(navigate.input_schema["properties"]["max_chars"].is_null());
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
    fn truncation_respects_small_character_budgets() {
        assert_eq!(truncate_chars("abc", 10), "abc");
        let truncated = truncate_chars("abcdefghij", 3);
        assert!(truncated.starts_with("abc"));
        assert_eq!(truncated.chars().count(), 3);
    }
}
