//! Playwright browser provider.
//!
//! Exposes one strict action-based model tool and maps every call onto the existing
//! Playwright bridge commands. The bridge
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

fn strict_object_schema(properties: Value, required: &[&str], constraints: Option<Value>) -> Value {
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    });
    if let Some(Value::Object(constraints)) = constraints {
        schema
            .as_object_mut()
            .expect("schema object")
            .extend(constraints);
    }
    schema
}

fn playwright_tool_specs() -> Vec<ToolSpec> {
    let target = json!({"target_id":{"type":"string","minLength":1}});
    let target_or_index = json!({
        "target_id":{"type":"string","minLength":1},
        "index":{"type":"integer","minimum":0}
    });
    let element = json!({
        "target_id": target["target_id"],
        "ref":{"type":"string","minLength":1,"description":"Element ref from action=observe_refs."},
        "selector":{"type":"string","minLength":1,"description":"CSS selector when no ref is available."}
    });
    let element_target = json!({"oneOf":[{"required":["ref"]},{"required":["selector"]}]});
    let drag_target = json!({"oneOf":[
        {"required":["from_ref","to_ref"]},
        {"required":["from_selector","to_selector"]}
    ]});
    let tab_target = json!({"oneOf":[{"required":["target_id"]},{"required":["index"]}]});

    let mut click = element.clone();
    click["action"] = json!({"const":"click"});
    click["double_click"] = json!({"type":"boolean"});
    let mut typing = element.clone();
    typing["action"] = json!({"const":"type"});
    typing["text"] = json!({"type":"string","minLength":1});
    typing["submit"] = json!({"type":"boolean"});
    let mut hover = element.clone();
    hover["action"] = json!({"const":"hover"});
    let mut select = element.clone();
    select["action"] = json!({"const":"select"});
    select["value"] = json!({"anyOf":[
        {"type":"string","minLength":1},
        {"type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":64}
    ]});

    vec![ToolSpec {
        name: TOOL_BROWSER.to_string(),
        title: None,
        description: "Browser automation. Select one action; observe refs before element actions. Text cursors require an unchanged page; batch has 1-10 dependent steps.".to_string(),
        input_schema: json!({"oneOf":[
            strict_object_schema(json!({"action":{"const":"navigate"},"url":{"type":"string","minLength":1,"description":"Absolute http(s) URL."},"target_id":target["target_id"],"timeout_ms":{"type":"integer","minimum":1,"maximum":120000}}), &["action","url"], None),
            strict_object_schema(json!({"action":{"const":"back"},"target_id":target["target_id"]}), &["action"], None),
            strict_object_schema(json!({"action":{"const":"tabs_list"}}), &["action"], None),
            strict_object_schema(json!({"action":{"const":"tabs_new"},"url":{"type":"string","minLength":1},"target_id":target["target_id"]}), &["action"], None),
            strict_object_schema(json!({"action":{"const":"tabs_select"},"target_id":target_or_index["target_id"],"index":target_or_index["index"]}), &["action"], Some(tab_target.clone())),
            strict_object_schema(json!({"action":{"const":"tabs_close"},"target_id":target_or_index["target_id"],"index":target_or_index["index"]}), &["action"], Some(tab_target)),
            strict_object_schema(json!({"action":{"const":"observe_refs"},"target_id":target["target_id"],"max_chars":{"type":"integer","minimum":1,"maximum":40000},"snapshot_timeout_ms":{"type":"integer","minimum":0,"maximum":120000}}), &["action"], None),
            strict_object_schema(json!({"action":{"const":"observe_text"},"target_id":target["target_id"],"max_chars":{"type":"integer","minimum":1,"maximum":40000},"cursor":{"type":"string","minLength":1}}), &["action"], None),
            strict_object_schema(json!({"action":{"const":"screenshot"},"target_id":target["target_id"],"path":{"type":"string","minLength":1,"description":"Workspace-relative output path."},"full_page":{"type":"boolean"}}), &["action"], None),
            strict_object_schema(click, &["action"], Some(element_target.clone())),
            strict_object_schema(typing, &["action","text"], Some(element_target.clone())),
            strict_object_schema(hover, &["action"], Some(element_target.clone())),
            strict_object_schema(select, &["action","value"], Some(element_target)),
            strict_object_schema(json!({"action":{"const":"drag"},"target_id":target["target_id"],"from_ref":{"type":"string","minLength":1},"from_selector":{"type":"string","minLength":1},"to_ref":{"type":"string","minLength":1},"to_selector":{"type":"string","minLength":1}}), &["action"], Some(drag_target)),
            strict_object_schema(json!({"action":{"const":"press"},"target_id":target["target_id"],"key":{"type":"string","minLength":1}}), &["action","key"], None),
            strict_object_schema(json!({"action":{"const":"wait"},"target_id":target["target_id"],"wait_ms":{"type":"integer","minimum":0,"maximum":120000},"load_state":{"type":"string","enum":["load","domcontentloaded","networkidle"]},"text":{"type":"string","minLength":1},"selector":{"type":"string","minLength":1},"state":{"type":"string","enum":["visible","hidden","attached","detached"]},"url":{"type":"string","minLength":1},"timeout_ms":{"type":"integer","minimum":1,"maximum":120000}}), &["action"], None),
            strict_object_schema(json!({"action":{"const":"batch"},"target_id":target["target_id"],"steps":{"type":"array","items":batch_step_schema(),"minItems":1,"maxItems":10}}), &["action","steps"], None)
        ]}),
    }]
}

fn batch_step_schema() -> Value {
    let element_target = json!({"oneOf":[{"required":["ref"]},{"required":["selector"]}]});
    let drag_target = json!({"oneOf":[
        {"required":["from_ref","to_ref"]},
        {"required":["from_selector","to_selector"]}
    ]});
    let element = |kind: &str| {
        json!({
            "kind":{"const":kind},
            "ref":{"type":"string","minLength":1},
            "selector":{"type":"string","minLength":1}
        })
    };
    let mut click = element("click");
    click["double_click"] = json!({"type":"boolean"});
    let mut typing = element("type");
    typing["text"] = json!({"type":"string","minLength":1});
    typing["submit"] = json!({"type":"boolean"});
    let hover = element("hover");
    let mut select = element("select");
    select["value"] = json!({"anyOf":[
        {"type":"string","minLength":1},
        {"type":"array","items":{"type":"string","minLength":1},"minItems":1,"maxItems":64}
    ]});
    json!({"oneOf":[
        strict_object_schema(json!({"kind":{"const":"navigate"},"url":{"type":"string","minLength":1},"timeout_ms":{"type":"integer","minimum":1,"maximum":120000}}), &["kind","url"], None),
        strict_object_schema(click, &["kind"], Some(element_target.clone())),
        strict_object_schema(typing, &["kind","text"], Some(element_target.clone())),
        strict_object_schema(hover, &["kind"], Some(element_target.clone())),
        strict_object_schema(select, &["kind","value"], Some(element_target)),
        strict_object_schema(json!({"kind":{"const":"drag"},"from_ref":{"type":"string","minLength":1},"from_selector":{"type":"string","minLength":1},"to_ref":{"type":"string","minLength":1},"to_selector":{"type":"string","minLength":1}}), &["kind"], Some(drag_target)),
        strict_object_schema(json!({"kind":{"const":"press"},"key":{"type":"string","minLength":1}}), &["kind","key"], None),
        strict_object_schema(json!({"kind":{"const":"wait"},"wait_ms":{"type":"integer","minimum":0,"maximum":120000},"load_state":{"type":"string","enum":["load","domcontentloaded","networkidle"]},"text":{"type":"string","minLength":1},"selector":{"type":"string","minLength":1},"state":{"type":"string","enum":["visible","hidden","attached","detached"]},"url":{"type":"string","minLength":1},"timeout_ms":{"type":"integer","minimum":1,"maximum":120000}}), &["kind"], None)
    ]})
}

/// Validate the single browser action again at execution time. Provider schemas
/// guide the model but are not an authorization boundary: callers may send raw
/// JSON directly, so action-specific fields, types and constraints are enforced
/// before any browser command reaches the bridge.
fn validate_browser_action(args: &Value) -> Result<String> {
    let map = args
        .as_object()
        .ok_or_else(|| anyhow!("browser arguments must be an object"))?;
    let action = required_str(args, "action")?;
    let allowed: &[&str] = match action.as_str() {
        "navigate" => &["action", "url", "target_id", "timeout_ms"],
        "back" => &["action", "target_id"],
        "tabs_list" => &["action"],
        "tabs_new" => &["action", "url", "target_id"],
        "tabs_select" | "tabs_close" => &["action", "target_id", "index"],
        "observe_refs" => &["action", "target_id", "max_chars", "snapshot_timeout_ms"],
        "observe_text" => &["action", "target_id", "max_chars", "cursor"],
        "screenshot" => &["action", "target_id", "path", "full_page"],
        "click" => &["action", "target_id", "ref", "selector", "double_click"],
        "type" => &["action", "target_id", "ref", "selector", "text", "submit"],
        "hover" => &["action", "target_id", "ref", "selector"],
        "select" => &["action", "target_id", "ref", "selector", "value"],
        "drag" => &[
            "action",
            "target_id",
            "from_ref",
            "from_selector",
            "to_ref",
            "to_selector",
        ],
        "press" => &["action", "target_id", "key"],
        "wait" => &[
            "action",
            "target_id",
            "wait_ms",
            "load_state",
            "text",
            "selector",
            "state",
            "url",
            "timeout_ms",
        ],
        "batch" => &["action", "target_id", "steps"],
        _ => return Err(anyhow!("unsupported browser action '{action}'")),
    };
    reject_unlisted_fields(map, allowed, &format!("browser action '{action}'"))?;
    validate_optional_strings(
        args,
        &[
            "target_id",
            "path",
            "url",
            "cursor",
            "ref",
            "selector",
            "from_ref",
            "from_selector",
            "to_ref",
            "to_selector",
            "text",
            "key",
        ],
    )?;
    validate_optional_boolean(args, &["double_click", "submit", "full_page"])?;
    validate_optional_integer(args, "max_chars", 1, 40_000)?;
    validate_optional_integer(args, "snapshot_timeout_ms", 0, 120_000)?;
    validate_optional_integer(args, "wait_ms", 0, 120_000)?;
    validate_optional_integer(args, "timeout_ms", 1, 120_000)?;
    validate_optional_integer(args, "index", 0, i64::MAX)?;
    validate_optional_enum(
        args,
        "load_state",
        &["load", "domcontentloaded", "networkidle"],
    )?;
    validate_optional_enum(
        args,
        "state",
        &["visible", "hidden", "attached", "detached"],
    )?;

    match action.as_str() {
        "navigate" => {
            required_str(args, "url")?;
        }
        "tabs_select" | "tabs_close" => require_tab_target(args, &action)?,
        "type" => {
            required_str(args, "text")?;
            require_element_target(args, &action)?;
        }
        "click" | "hover" => require_element_target(args, &action)?,
        "select" => {
            require_element_target(args, &action)?;
            validate_select_value(args, "browser action select")?;
        }
        "drag" => require_drag_target(args, "browser action drag")?,
        "press" => {
            required_str(args, "key")?;
        }
        "batch" => validate_batch_steps(
            args.get("steps")
                .ok_or_else(|| anyhow!("browser action batch requires steps"))?,
        )?,
        _ => {}
    }
    Ok(action)
}

fn reject_unlisted_fields(map: &Map<String, Value>, allowed: &[&str], subject: &str) -> Result<()> {
    if let Some(extra) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(anyhow!("field '{extra}' is not valid for {subject}"));
    }
    Ok(())
}

fn validate_optional_strings(args: &Value, keys: &[&str]) -> Result<()> {
    for key in keys {
        if args.get(*key).is_some() {
            required_str(args, key)?;
        }
    }
    Ok(())
}

fn validate_optional_boolean(args: &Value, keys: &[&str]) -> Result<()> {
    for key in keys {
        if args.get(*key).is_some_and(|value| !value.is_boolean()) {
            return Err(anyhow!("{key} must be a boolean"));
        }
    }
    Ok(())
}

fn validate_optional_integer(args: &Value, key: &str, min: i64, max: i64) -> Result<()> {
    let Some(value) = args.get(key) else {
        return Ok(());
    };
    let integer = value
        .as_i64()
        .ok_or_else(|| anyhow!("{key} must be an integer"))?;
    if !(min..=max).contains(&integer) {
        return Err(anyhow!("{key} is outside its allowed range"));
    }
    Ok(())
}

fn validate_optional_enum(args: &Value, key: &str, allowed: &[&str]) -> Result<()> {
    let Some(value) = args.get(key) else {
        return Ok(());
    };
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{key} must be a string"))?;
    if !allowed.contains(&value) {
        return Err(anyhow!("{key} has an unsupported value"));
    }
    Ok(())
}

fn require_element_target(args: &Value, subject: &str) -> Result<()> {
    let has_ref = opt_str(args, "ref").is_some();
    let has_selector = opt_str(args, "selector").is_some();
    if has_ref == has_selector {
        return Err(anyhow!(
            "browser action '{subject}' requires exactly one of ref or selector"
        ));
    }
    Ok(())
}

fn require_drag_target(args: &Value, subject: &str) -> Result<()> {
    let refs = opt_str(args, "from_ref").is_some() && opt_str(args, "to_ref").is_some();
    let selectors =
        opt_str(args, "from_selector").is_some() && opt_str(args, "to_selector").is_some();
    if refs == selectors {
        return Err(anyhow!(
            "{subject} requires exactly one complete ref or selector pair"
        ));
    }
    Ok(())
}

fn require_tab_target(args: &Value, action: &str) -> Result<()> {
    let has_target_id = opt_str(args, "target_id").is_some();
    let has_index = args.get("index").is_some();
    if has_target_id == has_index {
        return Err(anyhow!(
            "browser action '{action}' requires exactly one of target_id or index"
        ));
    }
    Ok(())
}

fn validate_select_value(args: &Value, subject: &str) -> Result<()> {
    let value = args
        .get("value")
        .ok_or_else(|| anyhow!("{subject} requires value"))?;
    let valid = value.as_str().is_some_and(|item| !item.trim().is_empty())
        || value.as_array().is_some_and(|items| {
            !items.is_empty()
                && items.len() <= 64
                && items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|text| !text.trim().is_empty()))
        });
    if !valid {
        return Err(anyhow!(
            "{subject} value must be a non-empty string or a 1-64 item non-empty string array"
        ));
    }
    Ok(())
}

fn validate_batch_steps(steps: &Value) -> Result<()> {
    let items = steps
        .as_array()
        .ok_or_else(|| anyhow!("browser action batch requires an array steps"))?;
    if items.is_empty() || items.len() > 10 {
        return Err(anyhow!("browser action batch requires 1-10 steps"));
    }
    for (index, step) in items.iter().enumerate() {
        let map = step
            .as_object()
            .ok_or_else(|| anyhow!("browser batch step {index} must be an object"))?;
        let kind = required_str(step, "kind")?;
        let allowed: &[&str] = match kind.as_str() {
            "navigate" => &["kind", "url", "timeout_ms"],
            "click" => &["kind", "ref", "selector", "double_click"],
            "type" => &["kind", "ref", "selector", "text", "submit"],
            "hover" => &["kind", "ref", "selector"],
            "select" => &["kind", "ref", "selector", "value"],
            "drag" => &["kind", "from_ref", "from_selector", "to_ref", "to_selector"],
            "press" => &["kind", "key"],
            "wait" => &[
                "kind",
                "wait_ms",
                "load_state",
                "text",
                "selector",
                "state",
                "url",
                "timeout_ms",
            ],
            _ => {
                return Err(anyhow!(
                    "browser batch step {index} has unsupported kind '{kind}'"
                ))
            }
        };
        reject_unlisted_fields(map, allowed, &format!("browser batch step {index}"))?;
        validate_optional_strings(
            step,
            &[
                "url",
                "ref",
                "selector",
                "text",
                "from_ref",
                "from_selector",
                "to_ref",
                "to_selector",
                "key",
            ],
        )?;
        validate_optional_boolean(step, &["double_click", "submit"])?;
        validate_optional_integer(step, "wait_ms", 0, 120_000)?;
        validate_optional_integer(step, "timeout_ms", 1, 120_000)?;
        validate_optional_enum(
            step,
            "load_state",
            &["load", "domcontentloaded", "networkidle"],
        )?;
        validate_optional_enum(
            step,
            "state",
            &["visible", "hidden", "attached", "detached"],
        )?;
        match kind.as_str() {
            "navigate" => {
                required_str(step, "url")?;
            }
            "click" | "hover" => require_element_target(step, &format!("batch step {index}"))?,
            "type" => {
                required_str(step, "text")?;
                require_element_target(step, &format!("batch step {index}"))?;
            }
            "select" => {
                require_element_target(step, &format!("batch step {index}"))?;
                validate_select_value(step, &format!("browser batch step {index}"))?;
            }
            "drag" => require_drag_target(step, &format!("browser batch step {index} drag"))?,
            "press" => {
                required_str(step, "key")?;
            }
            "wait" => {}
            _ => unreachable!("batch kind validated"),
        }
    }
    Ok(())
}

/// Map the one model-facing action tool to the internal bridge operations.
async fn dispatch(context: &ToolContext<'_>, tool: &str, args: &Value) -> Result<ProviderOutcome> {
    if tool.trim() != TOOL_BROWSER {
        return Err(anyhow!("Unknown browser tool '{tool}'"));
    }
    let action = validate_browser_action(args)?;
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
            let request = request_with("click", args, &["ref", "selector", "double_click"]);
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
        OP_HOVER => {
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
        OP_SELECT_OPTION => {
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
        OP_DRAG => {
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
        OP_PRESS_KEY => {
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
        copy_snapshot_args, normalize_image_mime, truncate_chars, validate_batch_steps,
        validate_browser_action, PlaywrightProvider, TOOL_BROWSER, TOOL_NAMES,
    };
    use crate::services::browser::provider::BrowserProvider;
    use serde_json::json;

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
    fn browser_schema_is_a_strict_action_union() {
        let schema = PlaywrightProvider::new()
            .tool_specs()
            .pop()
            .unwrap()
            .input_schema;
        let variants = schema["oneOf"].as_array().expect("action variants");
        assert!(variants.len() >= 17);
        for variant in variants {
            assert_eq!(variant["type"], json!("object"));
            assert_eq!(variant["additionalProperties"], json!(false));
            assert!(variant["properties"]["action"]["const"].is_string());
        }
        let select = variants
            .iter()
            .find(|variant| variant["properties"]["action"]["const"] == "tabs_select")
            .unwrap();
        assert_eq!(
            select["oneOf"],
            json!([{"required":["target_id"]},{"required":["index"]}])
        );
        let batch = variants
            .iter()
            .find(|variant| variant["properties"]["action"]["const"] == "batch")
            .unwrap();
        assert_eq!(batch["properties"]["steps"]["minItems"], 1);
        assert_eq!(batch["properties"]["steps"]["maxItems"], 10);
        let steps = batch["properties"]["steps"]["items"]["oneOf"]
            .as_array()
            .expect("strict batch variants");
        assert!(steps
            .iter()
            .all(|step| step["additionalProperties"] == false));
    }

    #[test]
    fn runtime_validation_rejects_cross_action_fields_and_bad_targets() {
        assert!(validate_browser_action(
            &json!({"action":"navigate","url":"https://example.test","ref":"e1"})
        )
        .is_err());
        assert!(validate_browser_action(&json!({"action":"click"})).is_err());
        assert!(validate_browser_action(
            &json!({"action":"drag","from_ref":"a","to_selector":"b"})
        )
        .is_err());
        assert!(validate_browser_action(&json!({"action":"tabs_close"})).is_err());
        assert!(
            validate_browser_action(&json!({"action":"observe_text","max_chars":40001})).is_err()
        );
        assert!(
            validate_browser_action(&json!({"action":"click","ref":"e1","selector":"#same"}))
                .is_err()
        );
        assert!(validate_browser_action(&json!({"action":"click","ref":"e1"})).is_ok());
        assert!(validate_browser_action(&json!({"action":"tabs_select","index":0})).is_ok());
        assert!(validate_browser_action(
            &json!({"action":"tabs_select","target_id":"tab-1","index":0})
        )
        .is_err());
    }

    #[test]
    fn batch_validation_is_bounded_and_forbids_nested_batch() {
        assert!(validate_batch_steps(&json!([{"kind":"batch","steps":[]}])).is_err());
        assert!(validate_batch_steps(
            &json!([{"kind":"click","ref":"e1","url":"https://example.test"}])
        )
        .is_err());
        assert!(
            validate_batch_steps(&json!([{"kind":"type","selector":"#field","text":"x"}])).is_ok()
        );
        let many: Vec<_> = (0..11)
            .map(|_| json!({"kind":"wait","wait_ms":1}))
            .collect();
        assert!(validate_batch_steps(&json!(many)).is_err());
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
}
