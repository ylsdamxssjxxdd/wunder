//! Playwright browser provider.
//!
//! Exposes one strict action-based model tool and maps every call onto the existing
//! Playwright bridge commands. The bridge
//! (`browser_bridge.py`) stays the driving engine; this provider only owns the
//! tool surface, the argument mapping and the content-block shaping.

use super::action_contract::{opt_str, required_str, validate_browser_action};
use super::preview::{preview_path_for_navigate, resolve_workspace_upload_files};
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

pub const TOOL_BROWSER: &str = "browser";

// Internal bridge operation names. These are not model-facing tools.
const OP_NAVIGATE: &str = "navigate";
const OP_NAVIGATE_BACK: &str = "back";
const OP_TABS: &str = "tabs";
const OP_SNAPSHOT: &str = "snapshot";
const OP_READ_PAGE: &str = "read_page";
const OP_CLICK: &str = "click";
const OP_TYPE: &str = "type";
const OP_HOVER: &str = "hover";
const OP_SELECT_OPTION: &str = "select";
const OP_DRAG: &str = "drag";
const OP_SCROLL: &str = "scroll";
const OP_CONSOLE: &str = "console";
const OP_RESIZE: &str = "resize";
const OP_FILE_UPLOAD: &str = "file_upload";
const OP_DIALOG: &str = "dialog";
const OP_NETWORK: &str = "network";
const OP_PRESS_KEY: &str = "press";
const OP_WAIT_FOR: &str = "wait";
const OP_SCREENSHOT: &str = "screenshot";
const OP_BATCH: &str = "batch";

/// Native tool names, in a stable order.
pub const TOOL_NAMES: &[&str] = &[TOOL_BROWSER];

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
        TOOL_NAMES.contains(&name.trim())
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

/// Model-facing action schema shared by browser providers. The Playwright and
/// Android WebView providers expose the same `browser` tool so switching the
/// backend does not change the model's tool surface.
pub fn playwright_tool_specs() -> Vec<ToolSpec> {
    // Flat action object with a single $defs block. The old surface repeated
    // every field across seventeen oneOf branches and carried a nested
    // ten-branch batch step schema. validate_browser_action remains the
    // runtime authority for per-action required fields, target constraints and
    // rejected cross-action keys, so the model surface does not need a union.
    let reference = |name: &str| json!({"$ref": format!("#/$defs/{name}")});
    let defs = json!({
        "x": {"type":"string","minLength":1},
        "i": {"type":"integer","minimum":0},
        "u": {"type":"string","minLength":1,"description":"Absolute http(s) URL. Not for workspace files: pass path instead."},
        "m": {"type":"integer","minimum":1,"maximum":120000},
        "h": {"type":"integer","minimum":1,"maximum":40000},
        "n": {"type":"integer","minimum":0,"maximum":120000},
        "p": {"type":"string","minLength":1,"description":"Workspace-relative path: for screenshot it names the output file (omit to use temp/browser/screenshots/browser_shot_<n>.<ext>); for navigate it is the workspace page to preview."},
        "f": {"type":"boolean"},
        "r": {"type":"string","minLength":1,"description":"Element ref from action=observe_refs."},
        "s": {"type":"string","minLength":1,"description":"CSS selector when no ref is available."},
        "v": {"anyOf":[
            {"type":"string","minLength":1},
            {"type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":64}
        ]},
        "l": {"type":"string","enum":["load","domcontentloaded","networkidle"]},
        "q": {"type":"string","enum":["visible","hidden","attached","detached"]},
        "d": {"type":"integer","minimum":-20000,"maximum":20000},
        "c": {"type":"integer","minimum":0,"maximum":20000,"description":"Viewport coordinate in CSS pixels."},
        "w": {"type":"integer","minimum":240,"maximum":3840,"description":"Logical viewport width in CSS pixels."},
        "t": {"type":"integer","minimum":240,"maximum":3840,"description":"Logical viewport height in CSS pixels."},
        "e": {"type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":16,"description":"Workspace-relative files to upload."}
    });
    vec![ToolSpec {
        name: TOOL_BROWSER.to_string(),
        title: None,
        description: "Built-in browser automation, already bundled — no install. Usual flow: navigate -> observe_refs -> click/type -> observe_text or screenshot, re-observing after changes. click/hover take one of ref, selector or visible text; type needs text (overwrites); select needs value (an option value or its visible label); press needs key. drag: from_ref+to_ref, or from_x/from_y/to_x/to_y for canvas orbit; scroll scrolls by delta_x/delta_y and also dispatches a wheel event at x/y so canvas pages receive it; console reads the tab's console output and uncaught errors; network lists recent requests (Playwright also records responses and failures); resize sets a logical CSS viewport (width/height are CSS pixels, the same unit as scroll deltas and click coordinates — screenshot images are rasterized at the device pixel ratio and may be scaled, so convert through the reported CSS viewport instead of subtracting scroll deltas from image pixels); file_upload takes workspace-relative files and either a ref/selector or a pending file chooser; dialog accepts/dismisses an alert/confirm/prompt (prompt_text fills prompt). timeout_ms defaults to 5s; wait needs wait_ms or a selector/text/url/load_state condition. navigate takes an absolute url, or path (workspace-relative) to preview a file through the internal wunder.preview origin with no server; a relative path passed as url is opened as a preview when that file exists, otherwise it errors with the url/path split. tabs_* manage tabs; target_id picks one. screenshot attaches a PNG plus a frame summary (blank/colors/luma). Observations default to 12000 chars.".to_string(),
        input_schema: json!({
            "type":"object",
            "$defs": defs,
            "properties": {
                "action": {
                    "type":"string",
                    "enum":["navigate","back","tabs_list","tabs_new","tabs_select","tabs_close","observe_refs","observe_text","screenshot","click","type","hover","select","drag","scroll","press","wait","console","resize","file_upload","dialog","network"],
                    "description":"One browser action per call; tabs_* actions manage tabs, observe_*/console/network read the page, resize/file_upload/dialog control browser state, the rest interact with it."
                },
                "url": reference("u"),
                "target_id": reference("x"),
                "timeout_ms": reference("m"),
                "index": reference("i"),
                "max_chars": reference("h"),
                "snapshot_timeout_ms": reference("n"),
                "cursor": reference("x"),
                "path": reference("p"),
                "full_page": reference("f"),
                "ref": reference("r"),
                "selector": reference("s"),
                "double_click": reference("f"),
                "text": reference("x"),
                "submit": reference("f"),
                "value": reference("v"),
                "from_ref": reference("r"),
                "from_selector": reference("s"),
                "to_ref": reference("r"),
                "to_selector": reference("s"),
                "from_x": reference("c"),
                "from_y": reference("c"),
                "to_x": reference("c"),
                "to_y": reference("c"),
                "delta_x": reference("d"),
                "delta_y": reference("d"),
                "x": reference("c"),
                "y": reference("c"),
                "clear": reference("f"),
                "key": reference("x"),
                "wait_ms": reference("n"),
                "load_state": reference("l"),
                "state": reference("q"),
                "width": reference("w"),
                "height": reference("t"),
                "files": reference("e"),
                "accept": reference("f"),
                "prompt_text": reference("x")
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    }]
}

/// Map the one model-facing action tool to the internal bridge operations.
async fn dispatch(context: &ToolContext<'_>, tool: &str, args: &Value) -> Result<ProviderOutcome> {
    if tool.trim() != TOOL_BROWSER {
        return Err(anyhow!("Unknown browser tool '{tool}'"));
    }
    let action = validate_browser_action(args)
        .map_err(|error| BrowserCommandFailure::invalid_argument(error.to_string()))?;
    let outcome = match action.as_str() {
        "navigate" => {
            let mut normalized = args.clone();
            normalized
                .as_object_mut()
                .expect("validated object")
                .insert("include_snapshot".to_string(), Value::Bool(false));
            dispatch_operation(context, OP_NAVIGATE, &normalized).await
        }
        "back" => dispatch_operation(context, OP_NAVIGATE_BACK, args).await,
        "tabs_list" => {
            dispatch_operation(context, OP_TABS, &with_internal_action(args, "list")).await
        }
        "tabs_new" => {
            dispatch_operation(context, OP_TABS, &with_internal_action(args, "new")).await
        }
        "tabs_select" => {
            dispatch_operation(context, OP_TABS, &with_internal_action(args, "select")).await
        }
        "tabs_close" => {
            dispatch_operation(context, OP_TABS, &with_internal_action(args, "close")).await
        }
        "observe_refs" => dispatch_operation(context, OP_SNAPSHOT, args).await,
        "observe_text" => dispatch_operation(context, OP_READ_PAGE, args).await,
        "screenshot" => dispatch_operation(context, OP_SCREENSHOT, args).await,
        "click" => dispatch_operation(context, OP_CLICK, args).await,
        "type" => dispatch_operation(context, OP_TYPE, args).await,
        "hover" => dispatch_operation(context, OP_HOVER, args).await,
        "select" => dispatch_operation(context, OP_SELECT_OPTION, args).await,
        "drag" => dispatch_operation(context, OP_DRAG, args).await,
        "scroll" => dispatch_operation(context, OP_SCROLL, args).await,
        "console" => dispatch_operation(context, OP_CONSOLE, args).await,
        "resize" => dispatch_operation(context, OP_RESIZE, args).await,
        "file_upload" => dispatch_operation(context, OP_FILE_UPLOAD, args).await,
        "dialog" => dispatch_operation(context, OP_DIALOG, args).await,
        "network" => dispatch_operation(context, OP_NETWORK, args).await,
        "press" => dispatch_operation(context, OP_PRESS_KEY, args).await,
        "wait" => dispatch_operation(context, OP_WAIT_FOR, args).await,
        "batch" => dispatch_operation(context, OP_BATCH, args).await,
        _ => unreachable!("validated browser action"),
    }?;
    Ok(stamp_browser_action(outcome, &action))
}

fn with_internal_action(args: &Value, operation: &str) -> Value {
    let mut normalized = args.clone();
    normalized
        .as_object_mut()
        .expect("validated object")
        .insert("action".to_string(), Value::String(operation.to_string()));
    normalized
}

fn stamp_browser_action(mut outcome: ProviderOutcome, action: &str) -> ProviderOutcome {
    let data = outcome.structured.get_or_insert_with(|| json!({}));
    if !data.is_object() {
        *data = json!({"result":data.clone()});
    }
    data.as_object_mut()
        .expect("object data")
        .insert("action".to_string(), Value::String(action.to_string()));
    outcome
}

async fn dispatch_operation(
    context: &ToolContext<'_>,
    operation: &str,
    args: &Value,
) -> Result<ProviderOutcome> {
    let service = browser_service(context.config);
    let scope = scope_from_context(context, args);
    let target = opt_str(args, "target_id");

    match operation {
        OP_NAVIGATE => {
            let mut call_args = Map::new();
            // 工作区页面走预览域名由 bridge 从磁盘直接应答：不需要自建 HTTP
            // 服务，也不依赖 file:// 是否被允许。把工作区相对路径填进 `url`
            // 是常见误用，文件确实存在就按预览打开，否则给出 url/path 分工。
            let root = context.workspace.workspace_root(context.workspace_id);
            let preview_path = preview_path_for_navigate(&root, args)?;
            if let Some(relative) = &preview_path {
                call_args.insert("preview_root".to_string(), json!(root.to_string_lossy()));
                call_args.insert("preview_path".to_string(), json!(relative));
            } else if let Some(url) = opt_str(args, "url") {
                call_args.insert("url".to_string(), json!(url));
            } else {
                return Err(anyhow!("Missing required parameter 'url'"));
            }
            copy_opt(args, &mut call_args, "timeout_ms");
            copy_opt(args, &mut call_args, "target_id");
            copy_snapshot_args(args, &mut call_args, true);
            let data = service
                .execute(&scope, "navigate", &Value::Object(call_args))
                .await?;
            let label = match preview_path {
                Some(relative) => format!("Opened workspace page {relative}."),
                None => format!("Navigated to {}.", opt_str(args, "url").unwrap_or_default()),
            };
            Ok(text_outcome(label, data))
        }
        OP_NAVIGATE_BACK => {
            let request = json!({"kind": "back"});
            let data = service
                .execute(&scope, "act", &act_args(request, target.as_deref()))
                .await?;
            Ok(text_outcome("Went back in history.", data))
        }
        OP_TABS => match opt_str(args, "action").as_deref() {
            Some("list") => {
                let data = service.execute(&scope, "tabs", &json!({})).await?;
                Ok(text_outcome("Open tabs:", data))
            }
            Some("new") => {
                let mut call_args = Map::new();
                copy_opt(args, &mut call_args, "url");
                copy_opt(args, &mut call_args, "target_id");
                let data = service
                    .execute(&scope, "open", &Value::Object(call_args))
                    .await?;
                Ok(text_outcome("Opened a tab.", data))
            }
            Some("select") => {
                let mut call_args = Map::new();
                copy_opt(args, &mut call_args, "target_id");
                copy_opt(args, &mut call_args, "index");
                let data = service
                    .execute(&scope, "focus", &Value::Object(call_args))
                    .await?;
                Ok(text_outcome("Selected a tab.", data))
            }
            Some("close") => {
                let mut call_args = Map::new();
                copy_opt(args, &mut call_args, "target_id");
                copy_opt(args, &mut call_args, "index");
                let data = service
                    .execute(&scope, "close", &Value::Object(call_args))
                    .await?;
                Ok(text_outcome("Closed a tab.", data))
            }
            _ => Err(anyhow!("unsupported browser tab operation")),
        },
        OP_SNAPSHOT => {
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
            Ok(ProviderOutcome::text(text).with_structured(data))
        }
        OP_READ_PAGE => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "max_chars");
            copy_opt(args, &mut call_args, "cursor");
            let data = service
                .execute(&scope, "read_page", &Value::Object(call_args))
                .await?;
            let text = data
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Ok(ProviderOutcome::text(text).with_structured(data))
        }
        OP_CLICK => {
            let request = request_with(
                "click",
                args,
                &["ref", "selector", "text", "double_click", "timeout_ms"],
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
        OP_TYPE => {
            let request = request_with(
                "type",
                args,
                &["ref", "selector", "text", "submit", "timeout_ms"],
            );
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
        OP_HOVER => {
            let request = request_with("hover", args, &["ref", "selector", "text", "timeout_ms"]);
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                "Hovered the element.",
            )
            .await
        }
        OP_SELECT_OPTION => {
            let request = request_with("select", args, &["ref", "selector", "value", "timeout_ms"]);
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                "Selected the option.",
            )
            .await
        }
        OP_DRAG => {
            let request = request_with(
                "drag",
                args,
                &[
                    "from_ref",
                    "from_selector",
                    "to_ref",
                    "to_selector",
                    "from_x",
                    "from_y",
                    "to_x",
                    "to_y",
                    "steps",
                    "timeout_ms",
                ],
            );
            act_outcome(service, &scope, request, target.as_deref(), "Dragged.").await
        }
        OP_SCROLL => {
            let request = request_with(
                "scroll",
                args,
                &["delta_x", "delta_y", "x", "y", "timeout_ms"],
            );
            act_outcome(service, &scope, request, target.as_deref(), "Scrolled.").await
        }
        OP_CONSOLE => {
            let request = request_with("console", args, &["clear"]);
            let data = service
                .execute(&scope, "act", &act_args(request, target.as_deref()))
                .await?;
            let text = data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(ProviderOutcome::text(text).with_structured(data))
        }
        OP_PRESS_KEY => {
            let key = required_str(args, "key")?;
            let request = request_with("press", args, &["key", "ref", "selector", "timeout_ms"]);
            act_outcome(
                service,
                &scope,
                request,
                target.as_deref(),
                format!("Pressed key {key}."),
            )
            .await
        }
        OP_WAIT_FOR => {
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
        OP_RESIZE => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "width");
            copy_opt(args, &mut call_args, "height");
            let data = service
                .execute(&scope, "resize", &Value::Object(call_args))
                .await?;
            let width = data.get("width").and_then(Value::as_i64).unwrap_or_default();
            let height = data.get("height").and_then(Value::as_i64).unwrap_or_default();
            Ok(text_outcome(format!("Resized viewport to {width}x{height}."), data))
        }
        OP_FILE_UPLOAD => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "ref");
            copy_opt(args, &mut call_args, "selector");
            copy_opt(args, &mut call_args, "timeout_ms");
            let root = context.workspace.workspace_root(context.workspace_id);
            let files = resolve_workspace_upload_files(&root, args)?;
            call_args.insert(
                "files".to_string(),
                json!(files
                    .iter()
                    .map(|path| path.to_string_lossy().to_string())
                    .collect::<Vec<_>>()),
            );
            let data = service
                .execute(&scope, "file_upload", &Value::Object(call_args))
                .await?;
            Ok(text_outcome(format!("Uploaded {} file(s).", files.len()), data))
        }
        OP_DIALOG => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "accept");
            copy_opt(args, &mut call_args, "prompt_text");
            let data = service
                .execute(&scope, "dialog", &Value::Object(call_args))
                .await?;
            let text = data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("Handled the dialog.")
                .to_string();
            Ok(ProviderOutcome::text(text).with_structured(data))
        }
        OP_NETWORK => {
            let mut call_args = Map::new();
            copy_opt(args, &mut call_args, "target_id");
            copy_opt(args, &mut call_args, "clear");
            let data = service
                .execute(&scope, "network", &Value::Object(call_args))
                .await?;
            let text = data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(ProviderOutcome::text(text).with_structured(data))
        }
        OP_BATCH => {
            let steps = args
                .get("steps")
                .cloned()
                .filter(Value::is_array)
                .ok_or_else(|| anyhow!("browser action batch requires an array 'steps'"))?;
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
        OP_SCREENSHOT => {
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
        other => Err(anyhow!("Unknown browser operation '{other}'")),
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

fn text_outcome(label: impl Into<String>, data: Value) -> ProviderOutcome {
    // Structured data already carries the observation; duplicating its JSON in
    // a text block wastes context and used to hide attachment loss.
    ProviderOutcome::text(label).with_structured(data)
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
        copy_snapshot_args, normalize_image_mime, truncate_chars, PlaywrightProvider, TOOL_BROWSER,
        TOOL_NAMES,
    };
    use crate::services::browser::provider::BrowserProvider;
    use crate::services::browser::providers::preview::{has_url_scheme, workspace_relative_path};
    use serde_json::{json, Value};

    #[test]
    fn provider_exposes_only_the_single_browser_tool() {
        let provider = PlaywrightProvider::new();
        let specs = provider.tool_specs();
        let names: Vec<String> = specs.iter().map(|spec| spec.name.clone()).collect();
        assert_eq!(names, vec![TOOL_BROWSER]);
        assert_eq!(TOOL_NAMES, &[TOOL_BROWSER]);
        assert!(provider.owns_tool(TOOL_BROWSER));
        for old_name in [
            "browser_navigate",
            "browser_observe",
            "browser_click",
            "browser_batch",
        ] {
            assert!(
                !provider.owns_tool(old_name),
                "old tool {old_name} must be unknown"
            );
        }
    }

    #[test]
    fn browser_schema_is_a_flat_action_object() {
        let schema = PlaywrightProvider::new()
            .tool_specs()
            .pop()
            .unwrap()
            .input_schema;
        assert!(schema["oneOf"].is_null());
        assert_eq!(schema["type"], json!("object"));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["required"], json!(["action"]));
        let actions = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        assert_eq!(actions.len(), 22);
        assert!(actions.iter().any(|action| action == "navigate"));
        assert!(actions.iter().any(|action| action == "scroll"));
        assert!(actions.iter().any(|action| action == "console"));
        assert!(actions.iter().any(|action| action == "resize"));
        assert!(actions.iter().any(|action| action == "file_upload"));
        assert!(actions.iter().any(|action| action == "dialog"));
        assert!(actions.iter().any(|action| action == "network"));
        assert!(!actions.iter().any(|action| action == "batch"));
        assert_eq!(
            schema["properties"]["target_id"]["$ref"],
            json!("#/$defs/x")
        );
        assert_eq!(schema["$defs"]["x"]["type"], json!("string"));
        assert!(schema["$defs"].get("b").is_none());
    }

    #[test]
    fn browser_schema_refs_resolve_and_survive_normalization() {
        let compact = PlaywrightProvider::new()
            .tool_specs()
            .pop()
            .expect("browser spec")
            .input_schema;
        let expanded = expand_local_refs_for_test(&compact);
        let compact_bytes = serde_json::to_vec(&compact)
            .expect("serialize compact browser schema")
            .len();
        let expanded_bytes = serde_json::to_vec(&expanded)
            .expect("serialize expanded browser schema")
            .len();
        eprintln!("browser schema compact={compact_bytes}, expanded={expanded_bytes}");

        // A flat action object references each definition only a few times,
        // so $defs now buys reuse and consistency rather than byte savings:
        // the two forms stay within a few percent. What must hold is that
        // every local reference resolves and normalization preserves them.
        assert!(
            compact_bytes <= expanded_bytes.saturating_mul(11) / 10,
            "compact={compact_bytes}, expanded={expanded_bytes}"
        );
        assert!(
            !serde_json::to_string(&expanded)
                .expect("serialize expanded browser schema")
                .contains("$ref"),
            "every local $ref must resolve"
        );
        let normalized = crate::core::json_schema::normalize_tool_input_schema(Some(&compact));
        assert_eq!(
            normalized["properties"]["target_id"]["$ref"],
            json!("#/$defs/x")
        );
        assert!(normalized["$defs"].get("b").is_none());
        assert!(normalized["$defs"]["v"].get("type").is_none());
        assert_eq!(
            normalized["$defs"]["v"]["anyOf"][1],
            json!({"type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":64})
        );
    }

    fn expand_local_refs_for_test(schema: &Value) -> Value {
        let defs = schema["$defs"].as_object().expect("local definitions");
        fn expand(value: &Value, defs: &serde_json::Map<String, Value>) -> Value {
            if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
                if let Some(name) = reference.strip_prefix("#/$defs/") {
                    return expand(defs.get(name).expect("known local definition"), defs);
                }
            }
            match value {
                Value::Array(items) => {
                    Value::Array(items.iter().map(|item| expand(item, defs)).collect())
                }
                Value::Object(map) => Value::Object(
                    map.iter()
                        .filter(|(key, _)| key.as_str() != "$defs")
                        .map(|(key, item)| (key.clone(), expand(item, defs)))
                        .collect(),
                ),
                _ => value.clone(),
            }
        }
        expand(schema, defs)
    }

    #[test]
    fn snapshot_default_can_be_explicitly_disabled() {
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

    #[test]
    fn workspace_preview_paths_stay_inside_the_workspace() {
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
    fn schemeless_urls_are_reported_as_paths_to_use_path() {
        // A workspace-relative path in `url` is the common mistake: it must be
        // detected before the driver answers "relative URL without a base".
        assert!(!has_url_scheme("temp/index.html"));
        assert!(!has_url_scheme("index.html"));
        assert!(!has_url_scheme("localhost:3000"));
        assert!(has_url_scheme("https://example.com"));
        assert!(has_url_scheme("http://127.0.0.1:8080/a.html"));
        assert!(has_url_scheme("about:blank"));
        assert!(has_url_scheme("data:text/html,hi"));
    }
}
