//! Phone control: the Android counterpart of the desktop-control tool.
//!
//! The runtime owns argument validation, result shaping and the screenshot
//! follow-up image. The device side (AccessibilityService, app launching,
//! clipboard, screenshots) is provided by a host installed by the Android
//! shell, so the engine stays platform-neutral and remains testable with a
//! fake host.

use super::{build_model_tool_success, ToolContext};
use crate::schemas::ToolSpec;
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TOOL_PHONE_CONTROL: &str = "手机控制";
pub const TOOL_PHONE_CONTROL_ALIAS: &str = "phone_control";
pub const TOOL_PHONE_CONTROL_ALIAS_SHORT: &str = "phone";

/// Coordinate space of the returned frame: the display fitted into this box
/// without upscaling and without changing its aspect. One coordinate unit is
/// one pixel of the screenshot the model is looking at — the same rule 桌面控制
/// uses — so both axes keep the same pixel size and no mental scaling is
/// needed. The shell does the fitting (`PhoneBridge.frameSize`); this value only
/// bounds the schema and supplies the fallback for replies that omit the size.
///
/// 1600 is the largest square the model view still accepts without rescaling
/// (50x50 patches of 32px, the `chat_media` budget), so a returned frame is
/// always exactly the grid the model reads coordinates from. Anything larger
/// would be silently downscaled on the way to the model and every tap would
/// drift.
const FRAME_BOX: i32 = 1600;
const MAX_TEXT_CHARS: usize = 16 * 1024;
const MAX_APP_CHARS: usize = 256;
const MAX_URL_CHARS: usize = 2048;
const MAX_WAIT_MS: u64 = 30_000;
const MAX_DURATION_MS: u64 = 30_000;
const DEFAULT_SWIPE_MS: u64 = 300;
const DEFAULT_LONG_PRESS_MS: u64 = 650;
const MAX_SCREENSHOT_BYTES: u64 = 8 * 1024 * 1024;
const HOST_TIMEOUT: Duration = Duration::from_secs(20);

const PRESS_KEYS: &[&str] = &["back", "home", "recents", "notifications", "quick_settings"];
const SETTINGS_PAGES: &[&str] = &[
    "settings",
    "wifi",
    "bluetooth",
    "display",
    "sound",
    "battery",
    "apps",
    "accessibility",
    "storage",
    "location",
];

/// One device-side executor installed by the Android shell. Calls arrive on a
/// blocking worker; implementations may block while the accessibility service
/// captures a frame or dispatches a gesture.
pub trait PhoneControlHost: Send + Sync {
    /// `args` carries touch points in the pixel space of the frame the model
    /// saw (`x`/`y`/`x1`…), which is the display fitted into [`FRAME_BOX`]; the
    /// shell scales them back to display pixels. Everything else is plain
    /// strings. The returned object follows the contract parsed by
    /// [`execute_with_host`] and must report `screen_width`/`screen_height`
    /// plus, once a frame exists, `normalized_width`/`normalized_height`.
    fn execute(&self, action: &str, args: &Value) -> Result<Value>;
}

static HOST: std::sync::RwLock<Option<Arc<dyn PhoneControlHost>>> = std::sync::RwLock::new(None);

/// The frame coordinate space reported by the latest device reply. Touch
/// arguments are checked against it, so a wrong guess is rejected together
/// with the real extent instead of being silently clamped to a screen edge.
static LAST_FRAME_NORM: std::sync::RwLock<Option<(i32, i32)>> = std::sync::RwLock::new(None);

fn last_frame_norm() -> Option<(i32, i32)> {
    *LAST_FRAME_NORM
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

fn remember_frame_norm(width: i32, height: i32) {
    if width > 0 && height > 0 {
        *LAST_FRAME_NORM
            .write()
            .unwrap_or_else(|error| error.into_inner()) = Some((width, height));
    }
}

/// A point outside the frame space the model was shown. Rejected rather than
/// clamped: a clamped guess becomes a tap on the screen border.
fn out_of_frame(point: (i32, i32), norm: (i32, i32)) -> bool {
    point.0 > norm.0 || point.1 > norm.1
}

/// Fit the display into [`FRAME_BOX`], keeping the aspect and never
/// upscaling. The shell reports the real value; this mirrors
/// `PhoneBridge.frameSize` only for replies that omit it.
fn frame_norm_size(screen_width: i32, screen_height: i32) -> (i32, i32) {
    if screen_width <= 0 || screen_height <= 0 {
        return (FRAME_BOX, FRAME_BOX);
    }
    if screen_width <= FRAME_BOX && screen_height <= FRAME_BOX {
        return (screen_width, screen_height);
    }
    if screen_width >= screen_height {
        let height = (i64::from(FRAME_BOX) * i64::from(screen_height) / i64::from(screen_width))
            .max(1) as i32;
        (FRAME_BOX, height)
    } else {
        let width = (i64::from(FRAME_BOX) * i64::from(screen_width) / i64::from(screen_height))
            .max(1) as i32;
        (width, FRAME_BOX)
    }
}

/// The shell installs the host at startup, after the Android activity exists
/// but before the engine can run a turn. A later install replaces the earlier
/// one so a recreated activity does not leave a stale context behind.
pub fn install_phone_control_host(host: Arc<dyn PhoneControlHost>) {
    let mut slot = HOST.write().unwrap_or_else(|error| error.into_inner());
    *slot = Some(host);
}

/// The tool is offered only when a device executor is installed. 桌面版, 命令行 and
/// 服务端 never install one, so the phone surface cannot leak into them.
pub fn phone_tools_enabled() -> bool {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .is_some()
}

fn phone_control_host() -> Option<Arc<dyn PhoneControlHost>> {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn is_phone_control_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if matches!(cleaned, TOOL_PHONE_CONTROL | "手机控制器" | "手机监视器") {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_PHONE_CONTROL_ALIAS
            | TOOL_PHONE_CONTROL_ALIAS_SHORT
            | "phone_controller"
            | "phone_monitor"
    )
}

pub fn phone_control_tool_spec(language: &str) -> ToolSpec {
    let t = |key: &str| crate::i18n::t_in_language(key, language);
    let bbox = json!({
        "type": "array",
        "items": {"type": "integer", "minimum": 0, "maximum": FRAME_BOX},
        "anyOf": [
            {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": FRAME_BOX}, "minItems": 2, "maxItems": 2},
            {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": FRAME_BOX}, "minItems": 4, "maxItems": 4}
        ],
        "description": t("tool.spec.phone_control.args.bbox")
    });
    let mut properties = serde_json::Map::new();
    properties.insert(
        "action".to_string(),
        json!({
            "type": "string",
            "enum": [
                "screenshot", "ui_tree", "tap", "long_press", "swipe", "type_text",
                "press_key", "open_app", "open_url", "open_settings", "list_apps",
                "device_info", "clipboard", "wait"
            ],
            "description": t("tool.spec.phone_control.args.action")
        }),
    );
    properties.insert("bbox".to_string(), bbox);
    properties.insert(
        "to_bbox".to_string(),
        json!({
            "type": "array",
            "items": {"type": "integer", "minimum": 0, "maximum": FRAME_BOX},
            "description": t("tool.spec.phone_control.args.to_bbox")
        }),
    );
    properties.insert(
        "text".to_string(),
        json!({
            "type": "string", "minLength": 1, "maxLength": MAX_TEXT_CHARS,
            "description": t("tool.spec.phone_control.args.text")
        }),
    );
    properties.insert(
        "key".to_string(),
        json!({
            "type": "string", "enum": PRESS_KEYS,
            "description": t("tool.spec.phone_control.args.key")
        }),
    );
    properties.insert(
        "app".to_string(),
        json!({
            "type": "string", "minLength": 1, "maxLength": MAX_APP_CHARS,
            "description": t("tool.spec.phone_control.args.app")
        }),
    );
    properties.insert(
        "url".to_string(),
        json!({
            "type": "string", "minLength": 1, "maxLength": MAX_URL_CHARS,
            "description": t("tool.spec.phone_control.args.url")
        }),
    );
    properties.insert(
        "page".to_string(),
        json!({
            "type": "string", "enum": SETTINGS_PAGES,
            "description": t("tool.spec.phone_control.args.page")
        }),
    );
    properties.insert(
        "operation".to_string(),
        json!({
            "type": "string", "enum": ["get", "set"],
            "description": t("tool.spec.phone_control.args.operation")
        }),
    );
    properties.insert(
        "delay_ms".to_string(),
        json!({
            "type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS,
            "description": t("tool.spec.phone_control.args.delay_ms")
        }),
    );
    properties.insert(
        "duration_ms".to_string(),
        json!({
            "type": "integer", "minimum": 0, "maximum": MAX_DURATION_MS,
            "description": t("tool.spec.phone_control.args.duration_ms")
        }),
    );
    // A flat action object keeps the first-turn tool budget small. Each
    // action's required fields are enforced by `parse_phone_args`, which
    // rejects missing or meaningless combinations before any device call.
    ToolSpec {
        name: TOOL_PHONE_CONTROL.to_string(),
        title: None,
        description: t("tool.spec.phone_control.description"),
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": ["action"],
            "additionalProperties": false
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhoneAction {
    Screenshot,
    UiTree,
    Tap,
    LongPress,
    Swipe,
    TypeText,
    PressKey,
    OpenApp,
    OpenUrl,
    OpenSettings,
    ListApps,
    DeviceInfo,
    Clipboard,
    Wait,
}

impl PhoneAction {
    fn from_raw(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "screenshot" => Some(Self::Screenshot),
            "ui_tree" => Some(Self::UiTree),
            "tap" => Some(Self::Tap),
            "long_press" => Some(Self::LongPress),
            "swipe" => Some(Self::Swipe),
            "type_text" => Some(Self::TypeText),
            "press_key" => Some(Self::PressKey),
            "open_app" => Some(Self::OpenApp),
            "open_url" => Some(Self::OpenUrl),
            "open_settings" => Some(Self::OpenSettings),
            "list_apps" => Some(Self::ListApps),
            "device_info" => Some(Self::DeviceInfo),
            "clipboard" => Some(Self::Clipboard),
            "wait" => Some(Self::Wait),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Screenshot => "screenshot",
            Self::UiTree => "ui_tree",
            Self::Tap => "tap",
            Self::LongPress => "long_press",
            Self::Swipe => "swipe",
            Self::TypeText => "type_text",
            Self::PressKey => "press_key",
            Self::OpenApp => "open_app",
            Self::OpenUrl => "open_url",
            Self::OpenSettings => "open_settings",
            Self::ListApps => "list_apps",
            Self::DeviceInfo => "device_info",
            Self::Clipboard => "clipboard",
            Self::Wait => "wait",
        }
    }

    /// Query actions do not change the screen, so their result is not a
    /// post-action observation and carries no follow-up frame.
    fn captures_frame(self) -> bool {
        !matches!(self, Self::DeviceInfo | Self::ListApps | Self::Clipboard)
    }
}

#[derive(Debug)]
struct PhoneArgs {
    action: PhoneAction,
    bbox: Option<(i32, i32)>,
    to_bbox: Option<(i32, i32)>,
    text: Option<String>,
    key: Option<String>,
    app: Option<String>,
    url: Option<String>,
    page: String,
    operation: Option<String>,
    delay_ms: u64,
    duration_ms: u64,
}

impl PhoneArgs {
    fn host_args(&self) -> Value {
        match self.action {
            PhoneAction::Tap => {
                let (x, y) = self.bbox.unwrap_or((0, 0));
                json!({"x": x, "y": y})
            }
            PhoneAction::LongPress => {
                let (x, y) = self.bbox.unwrap_or((0, 0));
                let duration = if self.duration_ms > 0 {
                    self.duration_ms
                } else {
                    DEFAULT_LONG_PRESS_MS
                };
                json!({"x": x, "y": y, "duration_ms": duration})
            }
            PhoneAction::Swipe => {
                let (x1, y1) = self.bbox.unwrap_or((0, 0));
                let (x2, y2) = self.to_bbox.unwrap_or((0, 0));
                let duration = if self.duration_ms > 0 {
                    self.duration_ms
                } else {
                    DEFAULT_SWIPE_MS
                };
                json!({"x1": x1, "y1": y1, "x2": x2, "y2": y2, "duration_ms": duration})
            }
            PhoneAction::TypeText => {
                let mut args = json!({"text": self.text.clone().unwrap_or_default()});
                if let Some((x, y)) = self.bbox {
                    args["x"] = json!(x);
                    args["y"] = json!(y);
                }
                args
            }
            PhoneAction::PressKey => json!({"key": self.key.clone().unwrap_or_default()}),
            PhoneAction::OpenApp => json!({"app": self.app.clone().unwrap_or_default()}),
            PhoneAction::OpenUrl => json!({"url": self.url.clone().unwrap_or_default()}),
            PhoneAction::OpenSettings => json!({"page": self.page}),
            PhoneAction::Clipboard => json!({
                "operation": self.operation.clone().unwrap_or_default(),
                "text": self.text.clone().unwrap_or_default(),
            }),
            PhoneAction::Wait => json!({"delay_ms": self.delay_ms}),
            PhoneAction::Screenshot
            | PhoneAction::UiTree
            | PhoneAction::ListApps
            | PhoneAction::DeviceInfo => json!({}),
        }
    }
}

fn parse_phone_args(args: &Value) -> Result<PhoneArgs> {
    let obj = args
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::prompt_t("tool.phone_control.invalid_args")))?;
    let action_raw = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::prompt_t("tool.phone_control.action_required")))?;
    let action = PhoneAction::from_raw(action_raw)
        .ok_or_else(|| anyhow!(crate::i18n::prompt_t("tool.phone_control.unknown_action")))?;
    let bbox = parse_bbox_field(obj.get("bbox"))?;
    let to_bbox = parse_bbox_field(obj.get("to_bbox"))?;
    let text = optional_string(obj, "text", MAX_TEXT_CHARS);
    let key = optional_string(obj, "key", 32);
    let app = optional_string(obj, "app", MAX_APP_CHARS);
    let url = optional_string(obj, "url", MAX_URL_CHARS);
    let operation = optional_string(obj, "operation", 8);
    let delay_ms = obj
        .get("delay_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(MAX_WAIT_MS);
    let duration_ms = obj
        .get("duration_ms")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(MAX_DURATION_MS);

    let requires_bbox = matches!(
        action,
        PhoneAction::Tap | PhoneAction::LongPress | PhoneAction::Swipe
    );
    if requires_bbox && bbox.is_none() {
        return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.bbox_required")));
    }
    if action == PhoneAction::Swipe && to_bbox.is_none() {
        return Err(anyhow!(crate::i18n::prompt_t(
            "tool.phone_control.to_bbox_required"
        )));
    }
    if action == PhoneAction::TypeText
        && text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.text_required")));
    }
    if action == PhoneAction::PressKey {
        let key = key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!(crate::i18n::prompt_t("tool.phone_control.key_required")))?;
        if !PRESS_KEYS.contains(&key) {
            return Err(anyhow!(format!(
                "{} ({})",
                crate::i18n::prompt_t("tool.phone_control.invalid_key"),
                PRESS_KEYS.join("/")
            )));
        }
    }
    if action == PhoneAction::OpenApp
        && app
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.app_required")));
    }
    if action == PhoneAction::OpenUrl {
        let value = url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!(crate::i18n::prompt_t("tool.phone_control.url_required")))?;
        if !(value.starts_with("https://") || value.starts_with("http://")) {
            return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.invalid_url")));
        }
    }
    let page = obj
        .get("page")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("settings")
        .to_string();
    if action == PhoneAction::OpenSettings && !SETTINGS_PAGES.contains(&page.as_str()) {
        return Err(anyhow!(crate::i18n::prompt_t(
            "tool.phone_control.invalid_settings_page"
        )));
    }
    let operation = operation.map(|value| value.to_ascii_lowercase());
    if action == PhoneAction::Clipboard {
        match operation.as_deref() {
            Some("get") => {}
            Some("set") => {
                if text.as_deref().unwrap_or("").is_empty() {
                    return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.text_required")));
                }
            }
            _ => {
                return Err(anyhow!(crate::i18n::prompt_t(
                    "tool.phone_control.operation_required"
                )))
            }
        }
    }

    Ok(PhoneArgs {
        action,
        bbox,
        to_bbox,
        text,
        key,
        app,
        url,
        page,
        operation,
        delay_ms,
        duration_ms,
    })
}

fn optional_string(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    max_chars: usize,
) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(max_chars).collect())
}

fn parse_bbox_field(value: Option<&Value>) -> Result<Option<(i32, i32)>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(items) = value.as_array() else {
        return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.invalid_bbox")));
    };
    let numbers = items
        .iter()
        .map(|item| item.as_i64().map(|value| value as i32))
        .collect::<Option<Vec<i32>>>();
    let Some(numbers) = numbers else {
        return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.invalid_bbox")));
    };
    let center = match numbers.as_slice() {
        [x, y] => (*x, *y),
        [x1, y1, x2, y2] => ((x1 + x2) / 2, (y1 + y2) / 2),
        _ => return Err(anyhow!(crate::i18n::prompt_t("tool.phone_control.invalid_bbox"))),
    };
    if numbers.iter().any(|value| *value < 0 || *value > FRAME_BOX) {
        return Err(anyhow!(crate::i18n::prompt_t(
            "tool.phone_control.bbox_out_of_range"
        )));
    }
    Ok(Some(center))
}

pub async fn tool_phone_control(_context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let host = phone_control_host()
        .ok_or_else(|| anyhow!(crate::i18n::prompt_t("tool.phone_control.disabled")))?;
    execute_with_host(host, args).await
}

async fn execute_with_host(host: Arc<dyn PhoneControlHost>, args: &Value) -> Result<Value> {
    let payload = parse_phone_args(args)?;
    // A coordinate outside the frame the model actually saw would be clamped
    // to the screen edge by the device; reject it with the real extent so the
    // model re-reads the screenshot instead of tapping the border.
    if let Some((norm_width, norm_height)) = last_frame_norm() {
        let norm = (norm_width, norm_height);
        if payload.bbox.is_some_and(|point| out_of_frame(point, norm))
            || payload
                .to_bbox
                .is_some_and(|point| out_of_frame(point, norm))
        {
            return Err(anyhow!(format!(
                "{} (frame {norm_width}x{norm_height})",
                crate::i18n::prompt_t("tool.phone_control.bbox_out_of_range")
            )));
        }
    }
    let action = payload.action;
    let host_args = payload.host_args();
    let started_at = Instant::now();
    let reply = crate::core::blocking::run_external_with_timeout(
        "phone_control",
        HOST_TIMEOUT,
        move || host.execute(action.as_str(), &host_args),
    )
    .await?;
    let reply = reply.as_object().cloned().unwrap_or_default();

    let ok = reply.get("ok").and_then(Value::as_bool).unwrap_or(true);
    let message = reply
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let code = reply
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let accessibility_connected = reply
        .get("accessibility_connected")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let foreground_app = reply
        .get("foreground_app")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let screen_width = reply
        .get("screen_width")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    let screen_height = reply
        .get("screen_height")
        .and_then(Value::as_i64)
        .unwrap_or(0) as i32;
    // The shell reports the coordinate space of the frame it just returned;
    // the fallback only covers a reply from an older shell.
    let (fallback_width, fallback_height) = frame_norm_size(screen_width, screen_height);
    let reply_norm = |key: &str| {
        reply
            .get(key)
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .clamp(0, i64::from(FRAME_BOX)) as i32
    };
    let (norm_width, norm_height) = match (
        reply_norm("normalized_width"),
        reply_norm("normalized_height"),
    ) {
        (width, height) if width > 0 && height > 0 => (width, height),
        _ => (fallback_width, fallback_height),
    };
    remember_frame_norm(norm_width, norm_height);
    let ui_tree = reply
        .get("ui_tree")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let settled = reply
        .get("settled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let screenshot_path = reply
        .get("screenshot_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let screenshot_bytes = screenshot_path
        .as_deref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .unwrap_or(0);

    let summary = if ok {
        phone_summary(action, &foreground_app)
    } else {
        localize_host_message(
            code,
            message.as_deref(),
            &crate::i18n::prompt_t("tool.phone_control.action_failed"),
        )
    };
    let followup_prompt = if action.captures_frame() && screenshot_path.is_some() {
        let key = if settled {
            "tool.phone_control.followup_prompt"
        } else {
            "tool.phone_control.followup_prompt_unsettled"
        };
        // Every followup names the frame size so the model keeps using the
        // pixel grid of the screenshot it is looking at, not a fixed square.
        Some(format!(
            "{} (frame {norm_width}x{norm_height} px)",
            crate::i18n::prompt_t(key)
        ))
    } else {
        None
    };

    let mut data = json!({
        "action": action.as_str(),
        "action_applied": ok,
        "state": if ok { "completed" } else { "failed" },
        "message": message.unwrap_or_default(),
        "accessibility_connected": accessibility_connected,
        "foreground_app": foreground_app,
        "screen_width": screen_width,
        "screen_height": screen_height,
        "normalized_width": norm_width,
        "normalized_height": norm_height,
        "elapsed_ms": started_at.elapsed().as_millis() as u64,
    });
    if let Some((x, y)) = payload.bbox {
        data["center_norm"] = json!([x, y]);
    }
    if let Some((x, y)) = payload.to_bbox {
        data["to_center_norm"] = json!([x, y]);
    }
    if !ui_tree.is_empty() {
        data["ui_tree"] = json!(ui_tree);
    }
    if let Some(path) = screenshot_path.as_deref() {
        data["screenshot_path"] = json!(path);
        data["screenshot_bytes"] = json!(screenshot_bytes);
    }
    if let Some(prompt) = followup_prompt.as_deref() {
        data["followup_prompt"] = json!(prompt);
    }
    if let Some(result) = reply.get("data").filter(|value| !value.is_null()) {
        data["result"] = result.clone();
    }
    if !ok {
        if let Some(code) = code {
            data["code"] = json!(code);
        }
    }

    Ok(build_model_tool_success(
        TOOL_PHONE_CONTROL_ALIAS,
        if ok { "completed" } else { "failed" },
        summary,
        data,
    ))
}

fn phone_summary(action: PhoneAction, foreground_app: &str) -> String {
    let action = action.as_str();
    if foreground_app.trim().is_empty() {
        crate::i18n::prompt_t_with_params(
            "tool.phone_control.summary",
            &std::collections::HashMap::from([("action".to_string(), action.to_string())]),
        )
    } else {
        crate::i18n::prompt_t_with_params(
            "tool.phone_control.summary_app",
            &std::collections::HashMap::from([
                ("action".to_string(), action.to_string()),
                ("app".to_string(), foreground_app.to_string()),
            ]),
        )
    }
}

/// Device-side failures use stable codes so the runtime can localize them;
/// unknown codes fall back to the host message, then to `fallback`.
fn localize_host_message(code: Option<&str>, message: Option<&str>, fallback: &str) -> String {
    let key = match code {
        Some("accessibility_disabled") => Some("tool.phone_control.accessibility_disabled"),
        Some("accessibility_disconnected") => Some("tool.phone_control.accessibility_disabled"),
        Some("no_editable_field") => Some("tool.phone_control.no_editable_field"),
        Some("app_not_found") => Some("tool.phone_control.app_not_found"),
        Some("screenshot_unsupported") => Some("tool.phone_control.screenshot_unsupported"),
        Some("screenshot_failed") => Some("tool.phone_control.capture_failed"),
        Some("clipboard_unavailable") => Some("tool.phone_control.clipboard_unavailable"),
        Some("invalid_target") => Some("tool.phone_control.invalid_bbox"),
        _ => None,
    };
    if let Some(key) = key {
        let localized = crate::i18n::prompt_t(key);
        if localized != key {
            return localized;
        }
    }
    message
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

pub async fn build_followup_user_message(result_data: &Value) -> Result<Option<Value>> {
    let Some(path) = result_data
        .get("screenshot_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let current_data_url = read_screenshot_data_url(&path).await?;
    let prompt = result_data
        .get("followup_prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| crate::i18n::prompt_t("tool.phone_control.followup_prompt"));
    let mut content = vec![json!({ "type": "text", "text": prompt })];
    content.push(json!({
        "type": "text",
        "text": crate::i18n::prompt_t_with_params(
            "tool.read_image.followup_path_hint",
            &std::collections::HashMap::from([(
                "path".to_string(),
                path.to_string_lossy().to_string(),
            )]),
        )
    }));
    content.push(json!({
        "type": "image_url",
        "image_url": { "url": current_data_url }
    }));
    Ok(Some(json!({
        "role": "user",
        "content": content
    })))
}

async fn read_screenshot_data_url(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| anyhow!(crate::i18n::prompt_t("tool.phone_control.capture_failed")))?;
    if bytes.len() as u64 > MAX_SCREENSHOT_BYTES {
        return Err(anyhow!(crate::i18n::prompt_t(
            "tool.phone_control.capture_too_large"
        )));
    }
    let (bytes, mime) = crate::core::blocking::run_cpu("phone_screenshot_view", move || {
        crate::services::chat_media::encode_model_ready_image_bytes(&bytes, "image/png")
    })
    .await?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(ok: bool, screenshot: Option<&str>) -> Value {
        json!({
            "ok": ok,
            "accessibility_connected": true,
            "screen_width": 1080,
            "screen_height": 2400,
            "normalized_width": 720,
            "normalized_height": 1600,
            "foreground_app": "com.example.settings",
            "ui_tree": "#0 Button [bounds=0,0,100,100]",
            "screenshot_path": screenshot,
            "settled": true,
        })
    }

    struct FakeHost {
        seen: std::sync::Mutex<Vec<(String, Value)>>,
        reply: Value,
    }

    impl PhoneControlHost for FakeHost {
        fn execute(&self, action: &str, args: &Value) -> Result<Value> {
            self.seen
                .lock()
                .unwrap()
                .push((action.to_string(), args.clone()));
            Ok(self.reply.clone())
        }
    }

    fn temp_png() -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        std::fs::write(file.path(), b"png-bytes").unwrap();
        file
    }

    #[tokio::test]
    async fn tap_maps_normalized_center_to_host_args() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: reply(true, None),
        });
        let result = execute_with_host(
            host.clone(),
            &json!({"action": "tap", "bbox": [100, 200, 300, 400]}),
        )
        .await
        .unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["data"]["action"], json!("tap"));
        assert_eq!(result["data"]["center_norm"], json!([200, 300]));
        // The frame space the shell reported, not a fixed square.
        assert_eq!(result["data"]["normalized_width"], json!(720));
        assert_eq!(result["data"]["normalized_height"], json!(1600));
        assert_eq!(result["data"]["screen_width"], json!(1080));
        let seen = host.seen.lock().unwrap();
        assert_eq!(seen[0].0, "tap");
        assert_eq!(seen[0].1, json!({"x": 200, "y": 300}));
    }

    #[tokio::test]
    async fn type_text_optional_bbox_is_forwarded() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: reply(true, None),
        });
        execute_with_host(
            host.clone(),
            &json!({"action": "type_text", "text": "hello", "bbox": [10, 20]}),
        )
        .await
        .unwrap();
        let seen = host.seen.lock().unwrap();
        assert_eq!(seen[0].1, json!({"text": "hello", "x": 10, "y": 20}));
    }

    #[test]
    fn out_of_range_bbox_is_rejected() {
        let error = parse_phone_args(&json!({"action": "tap", "bbox": [0, 2049]}))
            .expect_err("out of range");
        // The message is localized, so assert it is text rather than the key.
        let message = error.to_string();
        assert!(!message.is_empty());
        assert_ne!(message, "tool.phone_control.bbox_out_of_range");
    }

    #[test]
    fn frame_norm_size_keeps_the_screen_aspect() {
        // A 1080x2400 phone becomes a 720x1600 frame: same pixels per unit on
        // both axes, so a coordinate read off the screenshot is a pixel.
        assert_eq!(frame_norm_size(1080, 2400), (720, 1600));
        assert_eq!(frame_norm_size(2400, 1080), (1600, 720));
        // Frames already inside the box are reported 1:1 and never upscaled.
        assert_eq!(frame_norm_size(1080, 1920), (900, 1600));
        assert_eq!(frame_norm_size(1080, 1080), (1080, 1080));
        assert_eq!(frame_norm_size(0, 0), (FRAME_BOX, FRAME_BOX));
    }

    #[tokio::test]
    async fn reply_without_frame_size_falls_back_to_the_aspect_fit() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            // A shell that predates `normalized_*`: only the display size.
            reply: json!({"ok": true, "screen_width": 1080, "screen_height": 2400}),
        });
        let result = execute_with_host(host, &json!({"action": "screenshot"}))
            .await
            .unwrap();
        assert_eq!(result["data"]["normalized_width"], json!(720));
        assert_eq!(result["data"]["normalized_height"], json!(1600));
    }

    #[test]
    fn coordinates_outside_the_frame_are_rejected() {
        let norm = (720, 1600);
        assert!(!out_of_frame((0, 0), norm));
        assert!(!out_of_frame((720, 1600), norm));
        assert!(out_of_frame((1000, 500), norm));
        assert!(out_of_frame((500, 1700), norm));
    }

    #[test]
    fn frames_stay_inside_the_model_view_budget() {
        // A frame the model view would rescale breaks the one-pixel-per-unit
        // promise, so every resolution must land inside both limits.
        for (width, height) in [
            (1080, 2400),
            (2400, 1080),
            (1440, 3200),
            (2560, 1600),
            (1600, 1600),
            (3840, 2160),
            (720, 1280),
        ] {
            let (frame_width, frame_height) = frame_norm_size(width, height);
            assert!(frame_width <= FRAME_BOX && frame_height <= FRAME_BOX);
            assert!(
                crate::services::chat_media::model_view_size(
                    frame_width as u32,
                    frame_height as u32
                )
                .is_none(),
                "{}x{} becomes {}x{}, which the model view rescales",
                width,
                height,
                frame_width,
                frame_height
            );
        }
    }

    #[test]
    fn swipe_requires_target() {
        assert!(parse_phone_args(&json!({"action": "swipe", "bbox": [10, 10]})).is_err());
    }

    #[test]
    fn clipboard_set_requires_text() {
        assert!(parse_phone_args(&json!({"action": "clipboard", "operation": "set"})).is_err());
        assert!(parse_phone_args(&json!({"action": "clipboard", "operation": "get"})).is_ok());
    }

    #[test]
    fn open_url_rejects_non_http_schemes() {
        assert!(parse_phone_args(&json!({"action": "open_url", "url": "intent://x"})).is_err());
        assert!(
            parse_phone_args(&json!({"action": "open_url", "url": "https://example.com"})).is_ok()
        );
    }

    #[tokio::test]
    async fn query_actions_carry_no_followup_frame() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: reply(true, Some("/nonexistent/shot.png")),
        });
        let result = execute_with_host(host, &json!({"action": "device_info"}))
            .await
            .unwrap();
        assert!(result["data"].get("followup_prompt").is_none());
    }

    #[tokio::test]
    async fn screenshot_actions_expose_followup_frame() {
        let file = temp_png();
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: reply(true, Some(file.path().to_string_lossy().as_ref())),
        });
        let result = execute_with_host(host, &json!({"action": "tap", "bbox": [1, 2]}))
            .await
            .unwrap();
        assert_eq!(result["data"]["screenshot_bytes"], json!(9));
        assert!(result["data"]["followup_prompt"].is_string());
        let followup = build_followup_user_message(&result["data"])
            .await
            .unwrap()
            .expect("followup");
        let content = followup["content"].as_array().unwrap();
        assert!(content[1]["text"]
            .as_str()
            .unwrap()
            .contains("[image_path:"));
        assert!(content[2]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
    }

    #[tokio::test]
    async fn failed_action_is_reported_without_erroring() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: json!({
                "ok": false,
                "code": "accessibility_disabled",
                "message": "Accessibility service is not connected.",
                "accessibility_connected": false
            }),
        });
        let result = execute_with_host(host, &json!({"action": "tap", "bbox": [10, 10]}))
            .await
            .unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["data"]["action_applied"], json!(false));
        assert_eq!(result["data"]["state"], json!("failed"));
        assert_eq!(result["data"]["code"], json!("accessibility_disabled"));
    }

    #[test]
    fn aliases_resolve_to_phone_control() {
        assert!(is_phone_control_tool_name("手机控制"));
        assert!(is_phone_control_tool_name("phone_control"));
        assert!(is_phone_control_tool_name("PHONE"));
        assert!(!is_phone_control_tool_name("桌面控制"));
    }
}
