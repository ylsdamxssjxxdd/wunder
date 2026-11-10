use super::{build_model_tool_success, ToolContext};
use crate::config::{Config, DesktopControlConfig};
use crate::core::blocking;
use crate::schemas::ToolSpec;
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use image::ImageEncoder;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs;
use tokio::time::{sleep, Duration};
use tracing::warn;
use uuid::Uuid;

pub const TOOL_DESKTOP_CONTROL: &str = "桌面控制";
pub const TOOL_DESKTOP_CONTROL_ALIAS: &str = "desktop_control";
pub const TOOL_DESKTOP_CONTROL_ALIAS_SHORT: &str = "desktop";

const MAX_WAIT_MS: u64 = 30_000;
const MAX_MOVE_DURATION_MS: u64 = 30_000;
const MAX_SCROLL_STEPS: i32 = 100;
const MAX_INPUT_TEXT_CHARS: usize = 16 * 1024;
const MAX_KEY_CHARS: usize = 128;
const MAX_NORMALIZED_COORD: i32 = 10_000;
const MAX_SCREENSHOT_BYTES: u64 = 8 * 1024 * 1024;
// Give native menus and window state time to settle before the observation
// screenshot. The first delay is deliberately bounded; we then compare a
// small number of consecutive frames and stop as soon as the pixels settle.
// This avoids returning a stale frame while avoiding an unconditional long
// sleep for every action.
const MIN_OBSERVATION_SETTLE_MS: u64 = 320;
const OBSERVATION_PROBE_INTERVAL_MS: u64 = 120;
const OBSERVATION_QUIET_WINDOW_MS: u64 = 320;
const MAX_OBSERVATION_PROBES: usize = 10;
const MAX_OBSERVATION_DURATION_MS: u64 = 2_000;
fn strict_desktop_action_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// One strict action surface for model calls. Each `oneOf` branch exposes only
/// the fields meaningful to its action group; direct callers are checked again
/// by [`parse_desktop_control_args`] before native input is sent.
pub fn desktop_control_tool_spec(language: &str) -> ToolSpec {
    let t = |key: &str| crate::i18n::t_in_language(key, language);
    let bbox = json!({
        "type": "array",
        "items": {"type": "integer", "minimum": 0, "maximum": MAX_NORMALIZED_COORD},
        "anyOf": [
            {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": MAX_NORMALIZED_COORD}, "minItems": 2, "maxItems": 2},
            {"type": "array", "items": {"type": "integer", "minimum": 0, "maximum": MAX_NORMALIZED_COORD}, "minItems": 4, "maxItems": 4}
        ],
        "description": t("tool.spec.desktop_control.args.bbox")
    });
    let pointer_actions = json!([
        "left_click",
        "left_double_click",
        "right_click",
        "middle_click",
        "left_hold",
        "right_hold",
        "middle_hold",
        "left_release",
        "right_release",
        "middle_release"
    ]);
    let mut defs = serde_json::Map::new();
    defs.insert("bbox".to_string(), bbox);
    defs.insert(
        "scroll_steps".to_string(),
        json!({
            "type": "integer", "minimum": 1, "maximum": MAX_SCROLL_STEPS,
            "description": t("tool.spec.desktop_control.args.scroll_steps")
        }),
    );
    defs.insert(
        "key".to_string(),
        json!({
            "type": "string", "minLength": 1, "maxLength": MAX_KEY_CHARS,
            "description": t("tool.spec.desktop_control.args.key")
        }),
    );
    defs.insert(
        "text".to_string(),
        json!({
            "type": "string", "minLength": 1, "maxLength": MAX_INPUT_TEXT_CHARS,
            "description": t("tool.spec.desktop_control.args.text")
        }),
    );
    defs.insert(
        "delay_ms".to_string(),
        json!({
            "type": "integer", "minimum": 0, "maximum": MAX_WAIT_MS,
            "description": t("tool.spec.desktop_control.args.delay_ms")
        }),
    );
    defs.insert(
        "duration_ms".to_string(),
        json!({
            "type": "integer", "minimum": 0, "maximum": MAX_MOVE_DURATION_MS,
            "description": t("tool.spec.desktop_control.args.duration_ms")
        }),
    );
    let reference = |name: &str| json!({"$ref": format!("#/$defs/{name}")});
    let input_branch =
        |action: Value, mut fields: serde_json::Map<String, Value>, required: &[&str]| {
            fields.insert("action".to_string(), action);
            strict_desktop_action_schema(Value::Object(fields), required)
        };
    let base = || {
        let mut fields = serde_json::Map::new();
        fields.insert("bbox".to_string(), reference("bbox"));
        fields
    };
    let mut scroll = base();
    scroll.insert("scroll_steps".to_string(), reference("scroll_steps"));
    let mut press = base();
    press.insert("key".to_string(), reference("key"));
    // bbox stays available but optional: omitting it types into whatever
    // control already has focus, without a synthetic click that can move the
    // caret, select existing text or dismiss a menu.
    let mut typing = base();
    typing.insert("text".to_string(), reference("text"));
    let mut wait = base();
    wait.insert("delay_ms".to_string(), reference("delay_ms"));
    let mut move_mouse = base();
    move_mouse.insert("duration_ms".to_string(), reference("duration_ms"));
    let mut drag = base();
    drag.insert("to_bbox".to_string(), reference("bbox"));
    drag.insert("duration_ms".to_string(), reference("duration_ms"));

    ToolSpec {
        name: TOOL_DESKTOP_CONTROL.to_string(),
        title: None,
        description: t("tool.spec.desktop_control.description"),
        input_schema: json!({"type":"object", "$defs": defs, "oneOf": [
            input_branch(json!({"enum": pointer_actions}), base(), &["action", "bbox"]),
            input_branch(json!({"enum": ["scroll_down", "scroll_up"]}), scroll, &["action", "bbox"]),
            input_branch(json!({"const": "press_key"}), press, &["action", "key"]),
            input_branch(json!({"const": "type_text"}), typing, &["action", "text"]),
            input_branch(json!({"const": "wait"}), wait, &["action"]),
            input_branch(json!({"const": "move_mouse"}), move_mouse, &["action", "bbox"]),
            input_branch(json!({"const": "drag_drop"}), drag, &["action", "bbox", "to_bbox"])
        ]}),
    }
}

#[derive(Clone, Copy, Debug)]
struct BBox {
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
}

impl BBox {
    fn center(&self) -> (i32, i32) {
        let mut x1 = self.x1;
        let mut y1 = self.y1;
        let mut x2 = self.x2;
        let mut y2 = self.y2;
        if x1 > x2 {
            std::mem::swap(&mut x1, &mut x2);
        }
        if y1 > y2 {
            std::mem::swap(&mut y1, &mut y2);
        }
        ((x1 + x2) / 2, (y1 + y2) / 2)
    }

    /// A coordinate past the current normalized extent used to be silently
    /// clamped to the edge, which turned a wrong guess into a click on the
    /// screen border. Reject it instead, so the model can re-read
    /// `normalized_width`/`normalized_height` and retry.
    fn out_of_bounds(&self, norm_width: i32, norm_height: i32) -> bool {
        [self.x1, self.x2]
            .into_iter()
            .any(|value| value < 0 || value > norm_width)
            || [self.y1, self.y2]
                .into_iter()
                .any(|value| value < 0 || value > norm_height)
    }
}

#[derive(Clone, Copy, Debug)]
enum DesktopAction {
    LeftClick,
    LeftDoubleClick,
    RightClick,
    MiddleClick,
    LeftHold,
    RightHold,
    MiddleHold,
    LeftRelease,
    RightRelease,
    MiddleRelease,
    ScrollDown,
    ScrollUp,
    PressKey,
    TypeText,
    Wait,
    MoveMouse,
    DragDrop,
}

impl DesktopAction {
    fn from_raw(raw: &str) -> Option<Self> {
        match raw.trim() {
            "left_click" => Some(Self::LeftClick),
            "left_double_click" => Some(Self::LeftDoubleClick),
            "right_click" => Some(Self::RightClick),
            "middle_click" => Some(Self::MiddleClick),
            "left_hold" => Some(Self::LeftHold),
            "right_hold" => Some(Self::RightHold),
            "middle_hold" => Some(Self::MiddleHold),
            "left_release" => Some(Self::LeftRelease),
            "right_release" => Some(Self::RightRelease),
            "middle_release" => Some(Self::MiddleRelease),
            "scroll_down" => Some(Self::ScrollDown),
            "scroll_up" => Some(Self::ScrollUp),
            "press_key" => Some(Self::PressKey),
            "type_text" => Some(Self::TypeText),
            "wait" => Some(Self::Wait),
            "move_mouse" => Some(Self::MoveMouse),
            "drag_drop" => Some(Self::DragDrop),
            _ => None,
        }
    }
}

struct DesktopControlArgs {
    /// `wait` and `press_key` never move the pointer, so a coordinate is
    /// optional there; every pointer action still requires one.
    bbox: Option<BBox>,
    action: DesktopAction,
    action_raw: String,
    key: Option<String>,
    text: Option<String>,
    delay_ms: u64,
    duration_ms: u64,
    scroll_steps: i32,
    to_bbox: Option<BBox>,
}

struct DesktopScreenshot {
    path: PathBuf,
    download_url: String,
    norm_width: i32,
    norm_height: i32,
    size_bytes: usize,
    signature: FrameSignature,
}

/// A screen-space rectangle owned by this process. The agent window itself
/// continuously repaints while a turn is streaming; comparing those pixels
/// would report every observation as unsettled even when the controlled
/// application has finished redrawing.
#[derive(Clone, Copy, Debug)]
struct PixelRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl PixelRect {
    fn normalized(
        self,
        screen_width: i32,
        screen_height: i32,
        norm_width: usize,
        norm_height: usize,
    ) -> Option<(usize, usize, usize, usize)> {
        if screen_width <= 0 || screen_height <= 0 || self.width <= 0 || self.height <= 0 {
            return None;
        }
        let left = i64::from(self.x);
        let top = i64::from(self.y);
        let right = left + i64::from(self.width);
        let bottom = top + i64::from(self.height);
        let left = left.max(0);
        let top = top.max(0);
        let right = right.min(i64::from(screen_width));
        let bottom = bottom.min(i64::from(screen_height));
        if right <= left || bottom <= top {
            return None;
        }
        let scale_x = |value: i64| {
            (value * norm_width as i64 / i64::from(screen_width)).clamp(0, norm_width as i64)
                as usize
        };
        let scale_y = |value: i64| {
            (value * norm_height as i64 / i64::from(screen_height)).clamp(0, norm_height as i64)
                as usize
        };
        let x0 = scale_x(left).saturating_sub(2);
        let y0 = scale_y(top).saturating_sub(2);
        let x1 = scale_x(right).saturating_add(2).min(norm_width);
        let y1 = scale_y(bottom).saturating_add(2).min(norm_height);
        (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
    }
}

/// Coarse signature of one frame: the picture is cut into 16x16 cells and each
/// cell keeps its average colour.
///
/// Exact fingerprint equality looked correct but made every desktop that blinks
/// a caret, spins a progress indicator or ticks a clock report
/// `observation_stable: false` forever, and the model dutifully answered
/// "still changing, call wait" on a screen that was already settled. Comparing
/// cells tolerates those sub-percent animations while still catching a real
/// redraw.
const FRAME_CELL_PX: usize = 16;
/// Per-channel difference that still counts as the same cell; absorbs
/// antialiasing and lossy downscaling.
const FRAME_CELL_CHANNEL_TOLERANCE: i16 = 12;
/// Share of compared cells that may change while the frame still counts as
/// settled. 0.4% of a 1280x720 frame is about 14 cells.
const FRAME_STABLE_CHANGED_RATIO: f64 = 0.004;

#[derive(Clone, Default, PartialEq)]
struct FrameSignature {
    cols: usize,
    rows: usize,
    cells: Vec<Option<[u8; 3]>>,
}

impl FrameSignature {
    /// Share of comparable cells that differ. Masked cells are skipped, so the
    /// agent's own window cannot make the ratio meaningless.
    fn changed_ratio(&self, other: &Self) -> f64 {
        if self.cols != other.cols || self.rows != other.rows {
            return 1.0;
        }
        let mut compared = 0usize;
        let mut changed = 0usize;
        for (left, right) in self.cells.iter().zip(other.cells.iter()) {
            let (Some(left), Some(right)) = (left, right) else {
                continue;
            };
            compared += 1;
            if (0..3).any(|index| {
                (i16::from(left[index]) - i16::from(right[index])).abs()
                    > FRAME_CELL_CHANNEL_TOLERANCE
            }) {
                changed += 1;
            }
        }
        if compared == 0 {
            // Nothing is comparable (the agent window covers the whole screen):
            // never call a masked-out view settled.
            return 1.0;
        }
        changed as f64 / compared as f64
    }
}

fn frame_signature(
    rgba: &[u8],
    width: usize,
    height: usize,
    masks: &[(usize, usize, usize, usize)],
) -> FrameSignature {
    if width == 0 || height == 0 {
        return FrameSignature::default();
    }
    let cols = (width + FRAME_CELL_PX - 1) / FRAME_CELL_PX;
    let rows = (height + FRAME_CELL_PX - 1) / FRAME_CELL_PX;
    let inside = |x: usize, y: usize| {
        masks
            .iter()
            .any(|&(x0, y0, x1, y1)| x >= x0 && x < x1 && y >= y0 && y < y1)
    };
    let mut cells = Vec::with_capacity(cols.saturating_mul(rows));
    for row in 0..rows {
        for col in 0..cols {
            let x0 = col * FRAME_CELL_PX;
            let y0 = row * FRAME_CELL_PX;
            let x1 = (x0 + FRAME_CELL_PX).min(width);
            let y1 = (y0 + FRAME_CELL_PX).min(height);
            if inside((x0 + x1) / 2, (y0 + y1) / 2) {
                cells.push(None);
                continue;
            }
            let mut sums = [0u64; 3];
            let mut count = 0u64;
            for y in y0..y1 {
                let row_offset = y * width;
                for x in x0..x1 {
                    let offset = (row_offset + x) * 4;
                    sums[0] += u64::from(rgba[offset]);
                    sums[1] += u64::from(rgba[offset + 1]);
                    sums[2] += u64::from(rgba[offset + 2]);
                    count += 1;
                }
            }
            if count == 0 {
                cells.push(None);
                continue;
            }
            cells.push(Some([
                (sums[0] / count) as u8,
                (sums[1] / count) as u8,
                (sums[2] / count) as u8,
            ]));
        }
    }
    FrameSignature { cols, rows, cells }
}

#[derive(Default)]
struct FrameStabilityTracker {
    previous: Option<FrameSignature>,
    quiet_since_ms: u64,
    last_changed_ratio: f64,
}

impl FrameStabilityTracker {
    fn observe(&mut self, signature: &FrameSignature, elapsed_ms: u64) -> bool {
        let changed_ratio = match self.previous.as_ref() {
            Some(previous) => previous.changed_ratio(signature),
            None => 1.0,
        };
        self.last_changed_ratio = changed_ratio;
        if changed_ratio > FRAME_STABLE_CHANGED_RATIO {
            self.quiet_since_ms = elapsed_ms;
        }
        self.previous = Some(signature.clone());
        elapsed_ms.saturating_sub(self.quiet_since_ms) >= OBSERVATION_QUIET_WINDOW_MS
    }
}

pub fn is_desktop_control_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if matches!(cleaned, TOOL_DESKTOP_CONTROL | "桌面控制器" | "桌面监视器") {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_DESKTOP_CONTROL_ALIAS
            | TOOL_DESKTOP_CONTROL_ALIAS_SHORT
            | "desktop_controller"
            | "desktop_monitor"
    )
}

pub fn desktop_tools_enabled(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop")
        && config.tools.desktop_control.enabled
}

pub async fn tool_desktop_control(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    ensure_desktop_enabled(context.config)?;
    let payload = parse_desktop_control_args(args)?;
    let started_at = Instant::now();
    let config = context.config.tools.desktop_control.clone();

    let (screen_width, screen_height) = screen_size()?;
    // The configured width/height pair is a bounding box, not a target aspect
    // ratio: the observation keeps the screen aspect so the model judges
    // positions on an undistorted picture. A 1920x1080 desktop therefore
    // normalizes to 1280x720 instead of a stretched square, which also cuts
    // the image-token cost of every screenshot by roughly 44%.
    let (norm_width, norm_height) = fit_normalized_size(
        screen_width,
        screen_height,
        config.norm_width.max(1),
        config.norm_height.max(1),
    );

    if payload
        .bbox
        .as_ref()
        .is_some_and(|bbox| bbox.out_of_bounds(norm_width, norm_height))
    {
        return Err(anyhow!(format!(
            "{} (normalized {norm_width}x{norm_height})",
            crate::i18n::t("tool.desktop_control.bbox_out_of_range")
        )));
    }
    if payload
        .to_bbox
        .as_ref()
        .is_some_and(|target| target.out_of_bounds(norm_width, norm_height))
    {
        return Err(anyhow!(format!(
            "{} (normalized {norm_width}x{norm_height})",
            crate::i18n::t("tool.desktop_control.bbox_out_of_range")
        )));
    }
    let screen_max_x = (screen_width - 1).max(0);
    let screen_max_y = (screen_height - 1).max(0);
    let (cx, cy) = match payload.bbox.as_ref() {
        Some(bbox) => {
            let (cx_norm, cy_norm) = bbox.center();
            (
                map_coord(cx_norm.clamp(0, norm_width), norm_width, screen_max_x),
                map_coord(cy_norm.clamp(0, norm_height), norm_height, screen_max_y),
            )
        }
        None => (0, 0),
    };
    let wait_action = matches!(payload.action, DesktopAction::Wait);
    let wait_ms = payload.delay_ms.min(MAX_WAIT_MS);
    if wait_action {
        if wait_ms > 0 {
            sleep(Duration::from_millis(wait_ms)).await;
        }
    }

    match payload.action {
        DesktopAction::LeftClick => {
            mouse_click(MouseButton::Left, cx, cy)?;
        }
        DesktopAction::LeftDoubleClick => {
            mouse_double_click(MouseButton::Left, cx, cy)?;
        }
        DesktopAction::RightClick => {
            mouse_click(MouseButton::Right, cx, cy)?;
        }
        DesktopAction::MiddleClick => {
            mouse_click(MouseButton::Middle, cx, cy)?;
        }
        DesktopAction::LeftHold => {
            mouse_down(MouseButton::Left, cx, cy)?;
        }
        DesktopAction::RightHold => {
            mouse_down(MouseButton::Right, cx, cy)?;
        }
        DesktopAction::MiddleHold => {
            mouse_down(MouseButton::Middle, cx, cy)?;
        }
        DesktopAction::LeftRelease => {
            mouse_up(MouseButton::Left, cx, cy)?;
        }
        DesktopAction::RightRelease => {
            mouse_up(MouseButton::Right, cx, cy)?;
        }
        DesktopAction::MiddleRelease => {
            mouse_up(MouseButton::Middle, cx, cy)?;
        }
        DesktopAction::ScrollDown => {
            mouse_scroll(cx, cy, -payload.scroll_steps)?;
        }
        DesktopAction::ScrollUp => {
            mouse_scroll(cx, cy, payload.scroll_steps)?;
        }
        DesktopAction::PressKey => {
            let key = payload
                .key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.key_required")))?;
            send_key_sequence(key)?;
        }
        DesktopAction::TypeText => {
            let text = payload.text.as_deref().unwrap_or("").to_string();
            if text.trim().is_empty() {
                return Err(anyhow!(crate::i18n::t(
                    "tool.desktop_control.text_required"
                )));
            }
            // Only an explicit bbox clicks to focus. Most callers have just
            // clicked the field in a previous action; clicking it again moves
            // the caret to the bbox centre and can select or replace the text
            // that is already there.
            if payload.bbox.is_some() {
                mouse_click(MouseButton::Left, cx, cy)?;
                sleep(Duration::from_millis(30)).await;
            }
            // A long unicode typing run opens the X display or builds input
            // arrays for every character; keep it off the async workers.
            let text_to_send = text.clone();
            blocking::run_cpu("desktop_type_text", move || {
                send_unicode_text(&text_to_send)
            })
            .await?;
        }
        DesktopAction::Wait => {}
        DesktopAction::MoveMouse => {
            smooth_move(cx, cy, payload.duration_ms).await?;
        }
        DesktopAction::DragDrop => {
            let to_bbox = payload
                .to_bbox
                .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.to_bbox_required")))?;
            let (to_cx_norm_raw, to_cy_norm_raw) = to_bbox.center();
            let to_cx_norm = to_cx_norm_raw.clamp(0, norm_width);
            let to_cy_norm = to_cy_norm_raw.clamp(0, norm_height);
            let to_cx = map_coord(to_cx_norm, norm_width, screen_max_x);
            let to_cy = map_coord(to_cy_norm, norm_height, screen_max_y);
            mouse_down(MouseButton::Left, cx, cy)?;
            let drag_duration = if payload.duration_ms > 0 {
                payload.duration_ms
            } else {
                400
            };
            smooth_move(to_cx, to_cy, drag_duration).await?;
            mouse_up(MouseButton::Left, to_cx, to_cy)?;
        }
    }

    // Even an explicit wait may end while the application is still updating.
    // It skips only the initial action delay; the same quiet-window check
    // still prevents a fresh but unsettled frame from being called stable.
    let observation = capture_stable_screenshot(
        &config,
        !wait_action,
        Some((screen_width, screen_height)),
        (norm_width, norm_height),
    )
    .await;
    // Native input has already been delivered when the capture fails. Turning
    // that into a tool error would hide the applied action and invite the model
    // to repeat a click on a screen it never saw, so the two outcomes are
    // reported separately and the action stays truthful.
    let (
        screenshot,
        observation_settled_ms,
        observation_stable,
        observation_change_ratio,
        observation_error,
    ) = match observation {
        Ok((screenshot, settled_ms, stable, changed_ratio)) => {
            (Some(screenshot), settled_ms, stable, changed_ratio, None)
        }
        Err(err) => {
            warn!(
                "desktop observation failed after {}: {err}",
                payload.action_raw
            );
            (None, 0, false, 0.0, Some(err.to_string()))
        }
    };
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let (summary, prompt) = match screenshot.as_ref() {
        Some(_) => {
            let prompt_key = if !observation_stable {
                "tool.desktop_control.followup_prompt_unsettled"
            } else if wait_action {
                "tool.desktop_control.followup_prompt_wait"
            } else {
                "tool.desktop_control.followup_prompt"
            };
            let summary = if wait_action {
                format!("Waited {wait_ms}ms and captured a desktop screenshot.")
            } else {
                format!("Completed desktop action {}.", payload.action_raw)
            };
            (summary, crate::i18n::t(prompt_key))
        }
        None => (
            format!(
                "Desktop action {} was applied, but the screenshot failed.",
                payload.action_raw
            ),
            crate::i18n::t("tool.desktop_control.followup_prompt_no_frame"),
        ),
    };
    let mut data = json!({
        "action": payload.action_raw,
        "action_applied": true,
        "observation_stable": observation_stable,
        "observation_settled_ms": observation_settled_ms,
        "normalized_width": norm_width,
        "normalized_height": norm_height,
        "elapsed_ms": elapsed_ms,
    });
    if !observation_stable {
        // Tell the model how much actually moved: a caret flicker and a busy
        // progress bar both land here, but only one of them is worth a retry.
        data["observation_change_ratio"] =
            json!((observation_change_ratio * 1000.0).round() / 1000.0);
    }
    if let Some(error) = observation_error.as_deref() {
        data["observation_error"] = json!(error);
        // Without a frame there is no followup message, so the model only sees
        // this tool result: the recovery instruction has to travel inside it.
        data["next_step"] = json!(prompt);
    }
    if let Some(screenshot) = screenshot.as_ref() {
        // Intermediate stability probes have already been discarded. The model
        // receives only this latest post-action frame: the preceding frame
        // doubles vision context while making ordinary visual differences look
        // like an unconfirmed action result.
        cleanup_old_frames(&screenshot.path, config.max_frames.max(2));
        persist_screenshot_to_user_container(context, screenshot).await;
        data["screenshot_path"] = json!(screenshot.path.to_string_lossy().to_string());
        data["screenshot_artifact"] = json!({
            "id": screenshot.download_url,
            "mime": "image/png",
            "bytes": screenshot.size_bytes,
        });
        data["followup_prompt"] = json!(build_followup_prompt(
            prompt.clone(),
            screenshot.norm_width,
            screenshot.norm_height,
        ));
    }
    Ok(build_model_tool_success(
        "desktop_control",
        "completed",
        summary,
        data,
    ))
}

pub async fn build_followup_user_message(result_data: &Value) -> Result<Option<Value>> {
    // An action can be applied without producing a frame (capture failure);
    // there is nothing visual to attach and the tool result already says so.
    let Some(payload) = parse_followup_payload(result_data)? else {
        return Ok(None);
    };
    let current_data_url = read_screenshot_data_url(&payload.current_path).await?;
    let mut content = vec![json!({ "type": "text", "text": payload.prompt })];
    content.push(json!({
        "type": "image_url",
        "image_url": { "url": current_data_url }
    }));
    Ok(Some(json!({
        "role": "user",
        "content": content
    })))
}

fn parse_followup_payload(result_data: &Value) -> Result<Option<FollowupPayload>> {
    let obj = result_data
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.capture_failed")))?;
    let Some(current_path) = obj
        .get("screenshot_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let prompt = obj
        .get("followup_prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| crate::i18n::t("tool.desktop_control.followup_prompt"));
    Ok(Some(FollowupPayload {
        current_path: PathBuf::from(current_path),
        prompt,
    }))
}

async fn read_screenshot_data_url(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| anyhow!(crate::i18n::t("tool.desktop_control.capture_failed")))?;
    if bytes.len() as u64 > MAX_SCREENSHOT_BYTES {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_too_large"
        )));
    }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

struct FollowupPayload {
    current_path: PathBuf,
    prompt: String,
}

fn parse_desktop_control_args(args: &Value) -> Result<DesktopControlArgs> {
    let obj = args
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_args")))?;
    let action_raw = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.action_required")))?;
    let action = DesktopAction::from_raw(action_raw)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.unknown_action")))?;
    let allowed: &[&str] = match action {
        DesktopAction::PressKey => &["bbox", "action", "key"],
        DesktopAction::TypeText => &["bbox", "action", "text"],
        DesktopAction::Wait => &["bbox", "action", "delay_ms"],
        DesktopAction::MoveMouse => &["bbox", "action", "duration_ms"],
        DesktopAction::DragDrop => &["bbox", "action", "to_bbox", "duration_ms"],
        DesktopAction::ScrollDown | DesktopAction::ScrollUp => &["bbox", "action", "scroll_steps"],
        _ => &["bbox", "action"],
    };
    if let Some(extra) = obj.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(anyhow!(
            "field '{extra}' is not valid for desktop action '{action_raw}'"
        ));
    }
    let bbox = obj.get("bbox").map(parse_bbox).transpose()?;
    let delay_ms = optional_u64_field(obj, "delay_ms")?.unwrap_or(0);
    let duration_ms = optional_u64_field(obj, "duration_ms")?.unwrap_or(0);
    let scroll_steps = optional_i32_field(obj, "scroll_steps")?.unwrap_or(1);
    if delay_ms > MAX_WAIT_MS {
        return Err(anyhow!("delay_ms is outside its allowed range"));
    }
    if duration_ms > MAX_MOVE_DURATION_MS {
        return Err(anyhow!("duration_ms is outside its allowed range"));
    }
    if !(1..=MAX_SCROLL_STEPS).contains(&scroll_steps) {
        return Err(anyhow!("scroll_steps is outside its allowed range"));
    }
    let to_bbox = obj.get("to_bbox").map(parse_bbox).transpose()?;
    match action {
        DesktopAction::PressKey if required_text(obj, "key").is_none() => {
            return Err(anyhow!(crate::i18n::t("tool.desktop_control.key_required")));
        }
        DesktopAction::TypeText if required_text(obj, "text").is_none() => {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.text_required"
            )));
        }
        DesktopAction::DragDrop if to_bbox.is_none() => {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.to_bbox_required"
            )));
        }
        DesktopAction::Wait | DesktopAction::PressKey | DesktopAction::TypeText => {}
        _ if bbox.is_none() => {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.bbox_required"
            )));
        }
        _ => {}
    }
    Ok(DesktopControlArgs {
        bbox,
        action,
        action_raw: action_raw.to_string(),
        key: obj
            .get("key")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string()),
        text: obj
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        delay_ms,
        duration_ms,
        scroll_steps,
        to_bbox,
    })
}

fn required_text<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    let value = obj
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if key == "key" && value.chars().count() > MAX_KEY_CHARS {
        return None;
    }
    if key == "text" && value.chars().count() > MAX_INPUT_TEXT_CHARS {
        return None;
    }
    Some(value)
}

fn optional_u64_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>> {
    let Some(value) = obj.get(key) else {
        return Ok(None);
    };
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| anyhow!("{key} must be a non-negative integer"))
}

fn optional_i32_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Option<i32>> {
    let Some(value) = obj.get(key) else {
        return Ok(None);
    };
    value
        .as_i64()
        .and_then(|number| i32::try_from(number).ok())
        .map(Some)
        .ok_or_else(|| anyhow!("{key} must be an integer"))
}

fn parse_bbox(value: &Value) -> Result<BBox> {
    // Some function-call transports stringify non-scalar JSON arguments
    // before they reach the runtime. Keep the public schema strict (bbox is
    // an array), but recover this transport representation here so a valid
    // model call can still reach the native action dispatcher. Only a JSON
    // array is accepted: a free-form string never becomes a coordinate value.
    let decoded: Vec<Value>;
    let arr: &[Value] = match value {
        Value::Array(items) => items.as_slice(),
        Value::String(raw) => {
            decoded = serde_json::from_str::<Value>(raw.trim())
                .ok()
                .and_then(|parsed| match parsed {
                    Value::Array(items) => Some(items),
                    _ => None,
                })
                .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
            decoded.as_slice()
        }
        _ => return Err(anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox"))),
    };
    if arr.len() == 4 {
        let x1 = parse_i32(arr.first())
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
        let y1 = parse_i32(arr.get(1))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
        let x2 = parse_i32(arr.get(2))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
        let y2 = parse_i32(arr.get(3))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
        return bounded_bbox(x1, y1, x2, y2);
    }
    if arr.len() == 2 {
        let cx = parse_i32(arr.first())
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
        let cy = parse_i32(arr.get(1))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))?;
        return bounded_bbox(cx, cy, cx, cy);
    }
    Err(anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")))
}

fn bounded_bbox(x1: i32, y1: i32, x2: i32, y2: i32) -> Result<BBox> {
    if [x1, y1, x2, y2]
        .into_iter()
        .any(|value| !(0..=MAX_NORMALIZED_COORD).contains(&value))
    {
        return Err(anyhow!(crate::i18n::t("tool.desktop_control.invalid_bbox")));
    }
    Ok(BBox { x1, y1, x2, y2 })
}

fn parse_i32(value: Option<&Value>) -> Option<i32> {
    let value = value?;
    value.as_i64().and_then(|num| i32::try_from(num).ok())
}

fn ensure_desktop_enabled(config: &Config) -> Result<()> {
    if desktop_tools_enabled(config) {
        return Ok(());
    }
    Err(anyhow!(crate::i18n::t("tool.desktop_control.disabled")))
}

fn build_followup_prompt(base: String, norm_width: i32, norm_height: i32) -> String {
    if norm_width > 0 && norm_height > 0 {
        format!("{base} (normalized {norm_width}x{norm_height})")
    } else {
        base
    }
}

fn map_coord(value: i32, src_max: i32, dst_max: i32) -> i32 {
    if dst_max <= 0 {
        return 0;
    }
    if src_max <= 0 {
        return value.clamp(0, dst_max);
    }
    let clamped = value.clamp(0, src_max);
    let numerator = i64::from(clamped) * i64::from(dst_max) + i64::from(src_max) / 2;
    let mapped = (numerator / i64::from(src_max)) as i32;
    mapped.clamp(0, dst_max)
}

/// Fit the screen aspect into the configured normalization box.
///
/// The old behaviour stretched every screen into `norm_width x norm_height`,
/// so a 16:9 desktop arrived as a vertically stretched square: the model had
/// to judge positions on a distorted picture while paying for 1.78x the pixels
/// it needed. Keeping the aspect fixes both sides — the picture stays
/// recognisable and a 1920x1080 screen normalizes to 1280x720.
pub fn fit_normalized_size(
    screen_width: i32,
    screen_height: i32,
    box_width: i32,
    box_height: i32,
) -> (i32, i32) {
    let box_width = box_width.max(1);
    let box_height = box_height.max(1);
    if screen_width <= 0 || screen_height <= 0 {
        return (box_width, box_height);
    }
    // Integer cross-multiplication compares screen_w/box_w with screen_h/box_h
    // without floating point, so the result is exact for every resolution.
    let screen_landscape_ratio = i64::from(screen_width) * i64::from(box_height);
    let box_ratio = i64::from(screen_height) * i64::from(box_width);
    if screen_landscape_ratio >= box_ratio {
        let height = (i64::from(box_width) * i64::from(screen_height)
            + i64::from(screen_width) / 2)
            / i64::from(screen_width);
        let height = i32::try_from(height.max(1)).unwrap_or(box_height);
        (box_width, height.min(box_height))
    } else {
        let width = (i64::from(box_height) * i64::from(screen_width)
            + i64::from(screen_height) / 2)
            / i64::from(screen_height);
        let width = i32::try_from(width.max(1)).unwrap_or(box_width);
        (width.min(box_width), box_height)
    }
}

async fn capture_screenshot(
    timeout_ms: u64,
    expected_screen: Option<(i32, i32)>,
    norm: (i32, i32),
) -> Result<DesktopScreenshot> {
    let (norm_width, norm_height) = norm;
    let screenshot = match blocking::run_external_with_timeout(
        "tools.desktop_control.capture_screenshot",
        Duration::from_millis(timeout_ms.max(1)),
        move || {
            capture_screenshot_blocking(norm_width, norm_height, expected_screen)
                .map_err(anyhow::Error::msg)
        },
    )
    .await
    {
        Ok(value) => value,
        Err(err) if err.to_string().contains("timed out") => {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_timeout"
            )));
        }
        Err(err) => return Err(err),
    };
    Ok(screenshot)
}

/// Require a quiet interval rather than accepting the first identical pair:
/// applications can briefly keep an old frame before a delayed update begins.
/// Animated desktops still return the newest frame after bounded probes, with
/// a truthful unsettled flag so the caller waits instead of replaying input.
async fn capture_stable_screenshot(
    config: &DesktopControlConfig,
    delay_after_action: bool,
    expected_screen: Option<(i32, i32)>,
    norm: (i32, i32),
) -> Result<(DesktopScreenshot, u64, bool, f64)> {
    let started = Instant::now();
    if delay_after_action {
        sleep(Duration::from_millis(MIN_OBSERVATION_SETTLE_MS)).await;
    }
    // At least one frame is required to report an observation. Later probes
    // share one wall-clock budget instead of multiplying capture_timeout_ms
    // by the probe count on a slow desktop.
    let capture_timeout_ms = config.capture_timeout_ms.max(100);
    let mut previous = capture_screenshot(capture_timeout_ms, expected_screen, norm).await?;
    let mut tracker = FrameStabilityTracker::default();
    tracker.observe(&previous.signature, started.elapsed().as_millis() as u64);
    let mut stable = false;
    for _ in 0..MAX_OBSERVATION_PROBES {
        let Some(probe_timeout_ms) =
            observation_probe_timeout_ms(started.elapsed().as_millis() as u64, capture_timeout_ms)
        else {
            break;
        };
        sleep(Duration::from_millis(OBSERVATION_PROBE_INTERVAL_MS)).await;
        let current = match tokio::time::timeout(
            Duration::from_millis(probe_timeout_ms),
            capture_screenshot(probe_timeout_ms, expected_screen, norm),
        )
        .await
        {
            Ok(Ok(screenshot)) => screenshot,
            Ok(Err(err)) => {
                warn!("desktop observation probe skipped: {err}");
                break;
            }
            Err(_) => break,
        };
        stable = tracker.observe(&current.signature, started.elapsed().as_millis() as u64);
        // Probe images are only used for settling detection; retaining them
        // would consume the configured frame budget and evict real action
        // observations. Keep the newest image, which is the one returned.
        let obsolete_probe = previous.path.clone();
        previous = current;
        if obsolete_probe != previous.path {
            let _ = fs::remove_file(obsolete_probe).await;
        }
        if stable {
            break;
        }
    }
    Ok((
        previous,
        started.elapsed().as_millis() as u64,
        stable,
        tracker.last_changed_ratio,
    ))
}

fn observation_probe_timeout_ms(elapsed_ms: u64, capture_timeout_ms: u64) -> Option<u64> {
    MAX_OBSERVATION_DURATION_MS
        .checked_sub(elapsed_ms)?
        .checked_sub(OBSERVATION_PROBE_INTERVAL_MS)
        .filter(|remaining| *remaining > 0)
        .map(|remaining| remaining.min(capture_timeout_ms))
}

async fn persist_screenshot_to_user_container(
    context: &ToolContext<'_>,
    screenshot: &DesktopScreenshot,
) {
    let workspace_id = context
        .workspace
        .scoped_user_id_by_container(context.user_id, USER_PRIVATE_CONTAINER_ID);
    if let Err(err) = context.workspace.ensure_user_root(&workspace_id) {
        warn!(
            "desktop screenshot persist skipped: ensure user root failed user_id={} error={err}",
            context.user_id
        );
        return;
    }
    let safe_session = sanitize_session_id(context.session_id);
    let filename = screenshot
        .path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("desktop_shot_{}.png", Uuid::new_v4().simple()));
    let relative = format!("desktop_control/{safe_session}/{filename}");
    let dest = match context.workspace.resolve_path(&workspace_id, &relative) {
        Ok(path) => path,
        Err(err) => {
            warn!(
                "desktop screenshot persist skipped: resolve path failed user_id={} error={err}",
                context.user_id
            );
            return;
        }
    };
    let parent = dest.parent().map(Path::to_path_buf);
    if let Some(parent) = parent.as_deref() {
        if let Err(err) = fs::create_dir_all(parent).await {
            warn!(
                "desktop screenshot persist skipped: create dir failed user_id={} error={err}",
                context.user_id
            );
            return;
        }
    }
    if let Err(err) = fs::copy(&screenshot.path, &dest).await {
        warn!(
            "desktop screenshot persist skipped: copy failed user_id={} error={err}",
            context.user_id
        );
        return;
    }
    // The user container keeps a copy so a picture stays reachable after the
    // temp frames rotate, but nothing ever pruned this directory: one file per
    // action grows without bound across a long session. Apply the same
    // newest-N policy the temp frames already use.
    if let Some(parent) = parent.as_deref() {
        let keep = context.config.tools.desktop_control.max_frames.max(2);
        prune_dir_frames(parent, keep);
    }
}

fn cleanup_old_frames(path: &Path, max_frames: usize) {
    let Some(dir) = path.parent() else {
        return;
    };
    prune_dir_frames(dir, max_frames);
}

/// Keep only the newest `max_frames` files in `dir`.
fn prune_dir_frames(dir: &Path, max_frames: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta.modified().ok()?;
            Some((path, modified))
        })
        .collect();
    if files.len() <= max_frames {
        return;
    }
    files.sort_by_key(|(_, modified)| *modified);
    let excess = files.len().saturating_sub(max_frames);
    for (path, _) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

fn capture_screenshot_blocking(
    norm_width: i32,
    norm_height: i32,
    expected_screen: Option<(i32, i32)>,
) -> Result<DesktopScreenshot> {
    let (screen_width, screen_height, rgba) = capture_screen_rgba()?;
    // The action coordinates were mapped against the screen size sampled
    // before the action. If the desktop changed in between (resolution
    // switch, monitor hot-plug), that mapping is stale; fail fast instead of
    // returning a screenshot whose normalized space no longer matches the
    // click that was just sent.
    if let Some((expected_width, expected_height)) = expected_screen {
        if screen_width != expected_width || screen_height != expected_height {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.screen_changed"
            )));
        }
    }
    let resized = resize_rgba(
        &rgba,
        screen_width as u32,
        screen_height as u32,
        norm_width as u32,
        norm_height as u32,
    )?;
    let png = encode_png(&resized, norm_width as u32, norm_height as u32)?;
    if png.len() as u64 > MAX_SCREENSHOT_BYTES {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_too_large"
        )));
    }
    let masks: Vec<_> = own_window_rects()
        .into_iter()
        .filter_map(|rect| {
            rect.normalized(
                screen_width,
                screen_height,
                norm_width as usize,
                norm_height as usize,
            )
        })
        .collect();
    let signature = frame_signature(&resized, norm_width as usize, norm_height as usize, &masks);
    let dir = resolve_temp_dir()?.join("desktop_control");
    std::fs::create_dir_all(&dir)
        .map_err(|err| anyhow!(format!("create temp dir failed: {err}")))?;
    let filename = format!("desktop_shot_{}.png", Uuid::new_v4().simple());
    let path = dir.join(&filename);
    std::fs::write(&path, &png)
        .map_err(|err| anyhow!(format!("write screenshot failed: {err}")))?;
    let download_url = format!("/wunder/temp_dir/download?filename=desktop_control/{filename}");
    Ok(DesktopScreenshot {
        path,
        download_url,
        norm_width,
        norm_height,
        size_bytes: png.len(),
        signature,
    })
}

fn resolve_temp_dir() -> Result<PathBuf> {
    const TEMP_DIR_ROOT_ENV: &str = "WUNDER_TEMP_DIR_ROOT";
    if let Ok(value) = std::env::var(TEMP_DIR_ROOT_ENV) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_absolute() {
                return Ok(candidate);
            }
            let root = std::env::current_dir().map_err(|err| anyhow!(err))?;
            return Ok(root.join(candidate));
        }
    }
    let root = std::env::current_dir().map_err(|err| anyhow!(err))?;
    Ok(root.join("config").join("data").join("temp_dir"))
}

#[cfg(test)]
mod temp_dir_tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resolve_temp_dir_uses_shared_runtime_root() {
        let _guard = ENV_LOCK.lock().expect("environment lock");
        let root = std::env::temp_dir().join(format!(
            "wunder-desktop-temp-root-{}",
            Uuid::new_v4().simple()
        ));
        std::env::set_var("WUNDER_TEMP_DIR_ROOT", &root);
        assert_eq!(resolve_temp_dir().expect("resolve temp root"), root);
        std::env::remove_var("WUNDER_TEMP_DIR_ROOT");
    }
}

fn sanitize_session_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "default".to_string();
    }
    let sanitized = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn resize_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    norm_width: u32,
    norm_height: u32,
) -> Result<Vec<u8>> {
    let image = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.capture_failed")))?;
    let resized = image::imageops::resize(
        &image,
        norm_width,
        norm_height,
        image::imageops::FilterType::Triangle,
    );
    Ok(resized.into_raw())
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|err| anyhow!(format!("encode png failed: {err}")))?;
    Ok(buf)
}

fn screen_size() -> Result<(i32, i32)> {
    let (width, height) = screen_metrics()?;
    if width <= 0 || height <= 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok((width, height))
}

#[cfg(windows)]
fn screen_metrics() -> Result<(i32, i32)> {
    crate::core::dpi::init_process_dpi_awareness();
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    Ok((width, height))
}

#[cfg(target_os = "linux")]
fn screen_metrics() -> Result<(i32, i32)> {
    with_x11(|xlib, display, screen| unsafe {
        Ok((
            (xlib.XDisplayWidth)(display, screen),
            (xlib.XDisplayHeight)(display, screen),
        ))
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn screen_metrics() -> Result<(i32, i32)> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

#[cfg(windows)]
fn own_window_rects() -> Vec<PixelRect> {
    crate::core::dpi::init_process_dpi_awareness();
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Context {
        pid: u32,
        rects: Vec<PixelRect>,
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = &mut *(lparam as *mut Context);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != context.pid || IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let mut rect = std::mem::zeroed::<RECT>();
        if GetWindowRect(hwnd, &mut rect) != 0 {
            let width = rect.right.saturating_sub(rect.left);
            let height = rect.bottom.saturating_sub(rect.top);
            if width > 0 && height > 0 {
                context.rects.push(PixelRect {
                    x: rect.left,
                    y: rect.top,
                    width,
                    height,
                });
            }
        }
        1
    }

    let mut context = Context {
        pid: unsafe { GetCurrentProcessId() },
        rects: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(collect), &mut context as *mut Context as LPARAM);
    }
    context.rects
}

#[cfg(target_os = "linux")]
fn own_window_rects() -> Vec<PixelRect> {
    with_x11(|xlib, display, screen| unsafe {
        let pid = std::process::id() as std::ffi::c_ulong;
        let pid_atom = (xlib.XInternAtom)(
            display,
            b"_NET_WM_PID\0".as_ptr() as *const std::ffi::c_char,
            0,
        );
        if pid_atom == 0 {
            return Ok(Vec::new());
        }
        let root = (xlib.XRootWindow)(display, screen);
        let mut rects = Vec::new();
        collect_own_x11_windows(xlib, display, root, root, pid, pid_atom, 0, &mut rects);
        Ok(rects)
    })
    .unwrap_or_default()
}

/// Walk the X11 window tree and collect visible client windows whose
/// `_NET_WM_PID` matches this process. Top-level windows are usually WM
/// frames, so a bounded recursion is required to reach the client window.
#[cfg(target_os = "linux")]
unsafe fn collect_own_x11_windows(
    xlib: &x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    window: x11_dl::xlib::Window,
    root: x11_dl::xlib::Window,
    pid: std::ffi::c_ulong,
    pid_atom: x11_dl::xlib::Atom,
    depth: u8,
    rects: &mut Vec<PixelRect>,
) {
    let mut actual_type = 0 as x11_dl::xlib::Atom;
    let mut actual_format = 0i32;
    let mut item_count = 0 as std::ffi::c_ulong;
    let mut bytes_after = 0 as std::ffi::c_ulong;
    let mut property: *mut u8 = std::ptr::null_mut();
    let status = (xlib.XGetWindowProperty)(
        display,
        window,
        pid_atom,
        0,
        1,
        0,
        0,
        &mut actual_type,
        &mut actual_format,
        &mut item_count,
        &mut bytes_after,
        &mut property,
    );
    let mut owned = false;
    if status == 0
        && actual_type != 0
        && actual_format == 32
        && item_count >= 1
        && !property.is_null()
    {
        owned = *(property as *const std::ffi::c_ulong) == pid;
    }
    if !property.is_null() {
        (xlib.XFree)(property as *mut std::ffi::c_void);
    }
    if owned {
        let mut attributes: x11_dl::xlib::XWindowAttributes = std::mem::zeroed();
        if (xlib.XGetWindowAttributes)(display, window, &mut attributes) != 0
            && attributes.map_state == x11_dl::xlib::IsViewable
            && attributes.width > 0
            && attributes.height > 0
        {
            let mut absolute_x = 0i32;
            let mut absolute_y = 0i32;
            let mut child = 0 as x11_dl::xlib::Window;
            if (xlib.XTranslateCoordinates)(
                display,
                window,
                root,
                0,
                0,
                &mut absolute_x,
                &mut absolute_y,
                &mut child,
            ) != 0
            {
                rects.push(PixelRect {
                    x: absolute_x,
                    y: absolute_y,
                    width: attributes.width,
                    height: attributes.height,
                });
            }
        }
        return;
    }
    if depth >= 4 {
        return;
    }
    let mut root_return = 0 as x11_dl::xlib::Window;
    let mut parent_return = 0 as x11_dl::xlib::Window;
    let mut children: *mut x11_dl::xlib::Window = std::ptr::null_mut();
    let mut child_count = 0u32;
    if (xlib.XQueryTree)(
        display,
        window,
        &mut root_return,
        &mut parent_return,
        &mut children,
        &mut child_count,
    ) != 0
        && !children.is_null()
    {
        for index in 0..child_count as usize {
            collect_own_x11_windows(
                xlib,
                display,
                *children.add(index),
                root,
                pid,
                pid_atom,
                depth + 1,
                rects,
            );
        }
        (xlib.XFree)(children as *mut std::ffi::c_void);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn own_window_rects() -> Vec<PixelRect> {
    Vec::new()
}

#[cfg(windows)]
fn capture_screen_rgba() -> Result<(i32, i32, Vec<u8>)> {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
        DIB_RGB_COLORS, HBITMAP, HDC, RGBQUAD, SRCCOPY,
    };

    let (width, height) = screen_metrics()?;
    if width <= 0 || height <= 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }

    unsafe {
        let screen_dc: HDC = GetDC(HWND::default());
        if screen_dc == 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let mem_dc: HDC = CreateCompatibleDC(screen_dc);
        if mem_dc == 0 {
            ReleaseDC(HWND::default(), screen_dc);
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let bmp: HBITMAP = CreateCompatibleBitmap(screen_dc, width, height);
        if bmp == 0 {
            DeleteDC(mem_dc);
            ReleaseDC(HWND::default(), screen_dc);
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let old = SelectObject(mem_dc, bmp as _);
        let ok = BitBlt(
            mem_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            0,
            0,
            SRCCOPY | CAPTUREBLT,
        );
        if ok == 0 {
            SelectObject(mem_dc, old);
            DeleteObject(bmp as _);
            DeleteDC(mem_dc);
            ReleaseDC(HWND::default(), screen_dc);
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }; 1],
        };

        let mut buffer = vec![0u8; (width * height * 4) as usize];
        let scan = GetDIBits(
            mem_dc,
            bmp,
            0,
            height as u32,
            buffer.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old);
        DeleteObject(bmp as _);
        DeleteDC(mem_dc);
        ReleaseDC(HWND::default(), screen_dc);

        if scan == 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }

        for pixel in buffer.chunks_mut(4) {
            let b = pixel[0];
            let r = pixel[2];
            pixel[0] = r;
            pixel[2] = b;
            pixel[3] = 255;
        }
        Ok((width, height, buffer))
    }
}

#[cfg(target_os = "linux")]
fn capture_screen_rgba() -> Result<(i32, i32, Vec<u8>)> {
    with_x11(|xlib, display, screen| unsafe {
        let width = (xlib.XDisplayWidth)(display, screen);
        let height = (xlib.XDisplayHeight)(display, screen);
        if width <= 0 || height <= 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let root = (xlib.XRootWindow)(display, screen);
        let visual = (xlib.XDefaultVisual)(display, screen);
        if visual.is_null() {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let image = (xlib.XGetImage)(
            display,
            root,
            0,
            0,
            width as u32,
            height as u32,
            !0 as std::ffi::c_ulong,
            x11_dl::xlib::ZPixmap,
        );
        if image.is_null() {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let image_ref = &*image;
        let bytes_per_pixel = (image_ref.bits_per_pixel / 8) as usize;
        if bytes_per_pixel != 3 && bytes_per_pixel != 4 {
            (xlib.XDestroyImage)(image);
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        let bytes = std::slice::from_raw_parts(
            image_ref.data as *const u8,
            image_ref.bytes_per_line as usize * height as usize,
        );
        let little_endian = image_ref.byte_order == x11_dl::xlib::LSBFirst;
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in 0..height as usize {
            let line_start = row * image_ref.bytes_per_line as usize;
            for pixel in bytes[line_start..line_start + width as usize * bytes_per_pixel]
                .chunks_exact(bytes_per_pixel)
            {
                let value = if bytes_per_pixel == 4 {
                    if little_endian {
                        u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])
                    } else {
                        u32::from_be_bytes([pixel[0], pixel[1], pixel[2], pixel[3]])
                    }
                } else if little_endian {
                    u32::from(pixel[0]) | u32::from(pixel[1]) << 8 | u32::from(pixel[2]) << 16
                } else {
                    u32::from(pixel[2]) | u32::from(pixel[1]) << 8 | u32::from(pixel[0]) << 16
                };
                rgba.push(x11_color(value, (*visual).red_mask));
                rgba.push(x11_color(value, (*visual).green_mask));
                rgba.push(x11_color(value, (*visual).blue_mask));
                rgba.push(255);
            }
        }
        (xlib.XDestroyImage)(image);
        Ok((width, height, rgba))
    })
}

#[cfg(target_os = "linux")]
fn with_x11<T>(
    operation: impl FnOnce(&x11_dl::xlib::Xlib, *mut x11_dl::xlib::Display, i32) -> Result<T>,
) -> Result<T> {
    let xlib = x11_dl::xlib::Xlib::open().map_err(|error| anyhow!("无法加载 X11：{error}"))?;
    unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.unsupported_platform"
            )));
        }
        let screen = (xlib.XDefaultScreen)(display);
        let result = operation(&xlib, display, screen);
        (xlib.XCloseDisplay)(display);
        result
    }
}

#[cfg(target_os = "linux")]
fn with_xtest<T>(
    operation: impl FnOnce(
        &x11_dl::xlib::Xlib,
        &x11_dl::xtest::Xf86vmode,
        *mut x11_dl::xlib::Display,
        i32,
    ) -> Result<T>,
) -> Result<T> {
    let xlib = x11_dl::xlib::Xlib::open().map_err(|error| anyhow!("无法加载 X11：{error}"))?;
    let xtest = x11_dl::xtest::Xf86vmode::open()
        .map_err(|error| anyhow!("当前 X11 会话没有 XTest 扩展：{error}"))?;
    unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.unsupported_platform"
            )));
        }
        let screen = (xlib.XDefaultScreen)(display);
        let result = operation(&xlib, &xtest, display, screen);
        (xlib.XCloseDisplay)(display);
        result
    }
}

#[cfg(target_os = "linux")]
fn x11_color(value: u32, mask: std::ffi::c_ulong) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let field = (value as u64 & mask) >> shift;
    let max = mask >> shift;
    ((field * 255 + max / 2) / max) as u8
}

#[cfg(not(any(windows, target_os = "linux")))]
fn capture_screen_rgba() -> Result<(i32, i32, Vec<u8>)> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

#[derive(Clone, Copy)]
enum MouseButton {
    Left,
    Right,
    Middle,
}

fn mouse_click(button: MouseButton, x: i32, y: i32) -> Result<()> {
    mouse_down(button, x, y)?;
    mouse_up(button, x, y)?;
    Ok(())
}

fn mouse_double_click(button: MouseButton, x: i32, y: i32) -> Result<()> {
    mouse_click(button, x, y)?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    mouse_click(button, x, y)?;
    Ok(())
}

fn mouse_down(button: MouseButton, x: i32, y: i32) -> Result<()> {
    set_cursor_pos(x, y)?;
    match button {
        MouseButton::Left => send_mouse_event(MouseEvent::LeftDown),
        MouseButton::Right => send_mouse_event(MouseEvent::RightDown),
        MouseButton::Middle => send_mouse_event(MouseEvent::MiddleDown),
    }
}

fn mouse_up(button: MouseButton, x: i32, y: i32) -> Result<()> {
    set_cursor_pos(x, y)?;
    match button {
        MouseButton::Left => send_mouse_event(MouseEvent::LeftUp),
        MouseButton::Right => send_mouse_event(MouseEvent::RightUp),
        MouseButton::Middle => send_mouse_event(MouseEvent::MiddleUp),
    }
}

fn mouse_scroll(x: i32, y: i32, steps: i32) -> Result<()> {
    set_cursor_pos(x, y)?;
    send_mouse_wheel(steps)
}

async fn smooth_move(x: i32, y: i32, duration_ms: u64) -> Result<()> {
    if duration_ms == 0 {
        return set_cursor_pos(x, y);
    }
    let (sx, sy) = cursor_pos().unwrap_or((x, y));
    let steps = ((duration_ms as f64 / 1000.0) * 60.0).max(1.0) as i32;
    let step_ms = (duration_ms / steps.max(1) as u64).max(1);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let nx = (sx as f64 + (x - sx) as f64 * t).round() as i32;
        let ny = (sy as f64 + (y - sy) as f64 * t).round() as i32;
        set_cursor_pos(nx, ny)?;
        sleep(Duration::from_millis(step_ms)).await;
    }
    Ok(())
}

fn send_key_sequence(keys: &str) -> Result<()> {
    let parts: Vec<&str> = keys
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(anyhow!(crate::i18n::t("tool.desktop_control.key_required")));
    }
    let mut modifiers = Vec::new();
    let mut main = None;
    for part in parts {
        if let Some(modifier) = parse_modifier(part) {
            modifiers.push(modifier);
        } else if main.is_none() {
            main = Some(part);
        }
    }
    if let Some(main_key) = main {
        for m in &modifiers {
            key_down(*m)?;
        }
        send_key(main_key)?;
        for m in modifiers.iter().rev() {
            key_up(*m)?;
        }
        return Ok(());
    }
    if modifiers.is_empty() {
        return Err(anyhow!(crate::i18n::t("tool.desktop_control.key_required")));
    }
    for modifier in modifiers {
        key_down(modifier)?;
        key_up(modifier)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextSegment {
    Char(char),
    Enter,
    Tab,
}

/// Text fields receive newlines and tabs as key presses, not as Unicode
/// characters. Sending U+000A through a keysym lookup used to fail on X11 and
/// silently drop line breaks on Windows; splitting them out also collapses
/// CRLF into one Enter so pasted multi-line text does not double-submit.
fn text_input_segments(text: &str) -> Vec<TextSegment> {
    let mut segments = Vec::with_capacity(text.chars().count());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                segments.push(TextSegment::Enter);
            }
            '\n' => segments.push(TextSegment::Enter),
            '\t' => segments.push(TextSegment::Tab),
            _ => segments.push(TextSegment::Char(ch)),
        }
    }
    segments
}

#[cfg(windows)]
fn send_unicode_text(text: &str) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VK_RETURN, VK_TAB,
    };
    let keyboard_input = |vk: u16, scan: u16, flags: u32| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let mut inputs = Vec::new();
    for segment in text_input_segments(text) {
        match segment {
            TextSegment::Enter | TextSegment::Tab => {
                let vk = match segment {
                    TextSegment::Enter => VK_RETURN,
                    TextSegment::Tab => VK_TAB,
                    TextSegment::Char(_) => unreachable!(),
                };
                inputs.push(keyboard_input(vk, 0, 0));
                inputs.push(keyboard_input(vk, 0, KEYEVENTF_KEYUP));
            }
            TextSegment::Char(ch) => {
                // SendInput accepts UTF-16 code units. A scalar above U+FFFF
                // therefore needs two press/release pairs; casting directly to
                // u16 would lose the upper surrogate.
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units).iter().copied() {
                    inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE));
                    inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
    }
    for chunk in inputs.chunks(128) {
        let sent = unsafe {
            SendInput(
                chunk.len() as u32,
                chunk.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent != chunk.len() as u32 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
unsafe fn x11_keysym_keycode(
    xlib: &x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    name: &str,
) -> Result<u32> {
    let name = std::ffi::CString::new(name)
        .map_err(|_| anyhow!(crate::i18n::t("tool.desktop_control.key_required")))?;
    let keysym = (xlib.XStringToKeysym)(name.as_ptr());
    let keycode = (xlib.XKeysymToKeycode)(display, keysym);
    if keysym == 0 || keycode == 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok(u32::from(keycode))
}

#[cfg(target_os = "linux")]
unsafe fn x11_fake_keycode(
    xtest: &x11_dl::xtest::Xf86vmode,
    display: *mut x11_dl::xlib::Display,
    keycode: u32,
    pressed: bool,
) -> Result<()> {
    if (xtest.XTestFakeKeyEvent)(display, keycode, i32::from(pressed), 0) == 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
unsafe fn x11_fake_key(
    xlib: &x11_dl::xlib::Xlib,
    xtest: &x11_dl::xtest::Xf86vmode,
    display: *mut x11_dl::xlib::Display,
    name: &str,
    pressed: bool,
) -> Result<()> {
    let keycode = x11_keysym_keycode(xlib, display, name)?;
    x11_fake_keycode(xtest, display, keycode, pressed)
}

/// XStringToKeysym is a direct mapping only for characters represented by the
/// active XKB layout. Returns the keycode and keysym when the layout has a
/// printable key for `ch`; layout mappings that need AltGr or a non-default
/// group are deliberately left to the Unicode fallback.
#[cfg(target_os = "linux")]
unsafe fn x11_char_mapping(
    xlib: &x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    ch: char,
) -> Option<(u32, std::ffi::c_ulong)> {
    if !ch.is_ascii() {
        return None;
    }
    let name = std::ffi::CString::new(ch.to_string()).ok()?;
    let keysym = (xlib.XStringToKeysym)(name.as_ptr());
    if keysym == 0 {
        return None;
    }
    let keycode = (xlib.XKeysymToKeycode)(display, keysym);
    if keycode == 0 {
        return None;
    }
    Some((u32::from(keycode), keysym))
}

#[cfg(target_os = "linux")]
unsafe fn x11_send_unicode_codepoint(
    xlib: &x11_dl::xlib::Xlib,
    xtest: &x11_dl::xtest::Xf86vmode,
    display: *mut x11_dl::xlib::Display,
    ch: char,
) -> Result<()> {
    // GTK, Qt and common X11 input methods accept Ctrl+Shift+U followed by a
    // hexadecimal Unicode codepoint and Enter. XTest can synthesize that
    // sequence without a clipboard or an input-method dependency.
    x11_fake_key(xlib, xtest, display, "Control_L", true)?;
    x11_fake_key(xlib, xtest, display, "Shift_L", true)?;
    x11_fake_key(xlib, xtest, display, "u", true)?;
    x11_fake_key(xlib, xtest, display, "u", false)?;
    x11_fake_key(xlib, xtest, display, "Shift_L", false)?;
    x11_fake_key(xlib, xtest, display, "Control_L", false)?;
    for digit in unicode_input_codepoint(ch).chars() {
        let name = digit.to_string();
        x11_fake_key(xlib, xtest, display, &name, true)?;
        x11_fake_key(xlib, xtest, display, &name, false)?;
    }
    x11_fake_key(xlib, xtest, display, "Return", true)?;
    x11_fake_key(xlib, xtest, display, "Return", false)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_unicode_text(text: &str) -> Result<()> {
    let segments = text_input_segments(text);
    with_xtest(|xlib, xtest, display, _| unsafe {
        let mut pending = 0usize;
        for segment in segments {
            match segment {
                TextSegment::Enter => {
                    x11_fake_key(xlib, xtest, display, "Return", true)?;
                    x11_fake_key(xlib, xtest, display, "Return", false)?;
                }
                TextSegment::Tab => {
                    x11_fake_key(xlib, xtest, display, "Tab", true)?;
                    x11_fake_key(xlib, xtest, display, "Tab", false)?;
                }
                TextSegment::Char(ch) => match x11_char_mapping(xlib, display, ch) {
                    Some((keycode, keysym)) => {
                        // The unshifted and shifted level tell us whether a
                        // plain keycode would type the wrong character (e.g.
                        // uppercase letters and punctuation needed Shift).
                        let unshifted = (xlib.XkbKeycodeToKeysym)(display, keycode as u8, 0, 0);
                        let shifted = (xlib.XkbKeycodeToKeysym)(display, keycode as u8, 0, 1);
                        let needs_shift = shifted == keysym && unshifted != keysym;
                        let shift = if needs_shift {
                            Some(x11_keysym_keycode(xlib, display, "Shift_L")?)
                        } else {
                            None
                        };
                        if let Some(shift_code) = shift {
                            x11_fake_keycode(xtest, display, shift_code, true)?;
                        }
                        x11_fake_keycode(xtest, display, keycode, true)?;
                        x11_fake_keycode(xtest, display, keycode, false)?;
                        if let Some(shift_code) = shift {
                            x11_fake_keycode(xtest, display, shift_code, false)?;
                        }
                    }
                    None => {
                        x11_send_unicode_codepoint(xlib, xtest, display, ch)?;
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                },
            }
            pending += 1;
            if pending >= 32 {
                (xlib.XFlush)(display);
                pending = 0;
            }
        }
        (xlib.XFlush)(display);
        Ok(())
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn send_unicode_text(_text: &str) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

fn build_vk_for_key(key: &str) -> Option<VkCode> {
    let key = key.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let named = match key.as_str() {
        "enter" | "return" => Some(0x0D),
        "tab" => Some(0x09),
        "esc" | "escape" => Some(0x1B),
        "backspace" | "back" => Some(0x08),
        "delete" | "del" => Some(0x2E),
        "insert" | "ins" => Some(0x2D),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pageup" | "pgup" => Some(0x21),
        "pagedown" | "pgdn" => Some(0x22),
        "left" | "arrowleft" => Some(0x25),
        "right" | "arrowright" => Some(0x27),
        "up" | "arrowup" => Some(0x26),
        "down" | "arrowdown" => Some(0x28),
        "space" => Some(0x20),
        "win" | "meta" | "super" => Some(0x5B),
        _ => None,
    };
    if let Some(vk) = named {
        return Some(VkCode::Raw(vk as u16));
    }
    if key.starts_with('f') && key.len() <= 3 {
        if let Ok(num) = key[1..].parse::<u8>() {
            if (1..=24).contains(&num) {
                return Some(VkCode::Raw(0x70 + (num as u16 - 1)));
            }
        }
    }
    if key.len() == 1 {
        let ch = key.chars().next().unwrap();
        if ch.is_ascii_alphanumeric() {
            return Some(VkCode::Raw(ch.to_ascii_uppercase() as u16));
        }
    }
    None
}

fn parse_modifier(key: &str) -> Option<VkCode> {
    match key.trim().to_lowercase().as_str() {
        "ctrl" | "control" => Some(VkCode::Control),
        "shift" => Some(VkCode::Shift),
        "alt" => Some(VkCode::Alt),
        "win" | "meta" | "super" => Some(VkCode::LWin),
        _ => None,
    }
}

fn send_key(key: &str) -> Result<()> {
    let vk = build_vk_for_key(key)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.key_required")))?;
    key_down(vk)?;
    key_up(vk)?;
    Ok(())
}

fn key_down(key: VkCode) -> Result<()> {
    send_key_event(key, false)
}

fn key_up(key: VkCode) -> Result<()> {
    send_key_event(key, true)
}

#[cfg(target_os = "linux")]
fn unicode_input_codepoint(ch: char) -> String {
    format!("{:x}", ch as u32)
}

#[cfg(all(test, target_os = "linux"))]
mod unicode_input_tests {
    use super::unicode_input_codepoint;

    #[test]
    fn unicode_codepoint_uses_lowercase_hex_without_prefix() {
        assert_eq!(unicode_input_codepoint('中'), "4e2d");
        assert_eq!(unicode_input_codepoint('😀'), "1f600");
    }
}

#[derive(Clone, Copy)]
enum VkCode {
    Control,
    Shift,
    Alt,
    LWin,
    Raw(u16),
}

#[cfg(windows)]
fn send_key_event(key: VkCode, key_up: bool) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
        VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT,
    };
    let vk: u16 = match key {
        VkCode::Control => VK_CONTROL,
        VkCode::Shift => VK_SHIFT,
        VkCode::Alt => VK_MENU,
        VkCode::LWin => VK_LWIN,
        VkCode::Raw(value) => value,
    };
    let mut flags = if key_up { KEYEVENTF_KEYUP } else { 0 };
    if is_extended_key(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_key_event(key: VkCode, key_up: bool) -> Result<()> {
    let name = match key {
        VkCode::Control => "Control_L".to_string(),
        VkCode::Shift => "Shift_L".to_string(),
        VkCode::Alt => "Alt_L".to_string(),
        VkCode::LWin => "Super_L".to_string(),
        VkCode::Raw(value) => x11_key_name(value)
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_control.key_required")))?,
    };
    let name = std::ffi::CString::new(name)
        .map_err(|_| anyhow!(crate::i18n::t("tool.desktop_control.key_required")))?;
    with_xtest(|xlib, xtest, display, _| unsafe {
        let keysym = (xlib.XStringToKeysym)(name.as_ptr());
        let keycode = (xlib.XKeysymToKeycode)(display, keysym);
        if keysym == 0
            || keycode == 0
            || (xtest.XTestFakeKeyEvent)(display, keycode as u32, i32::from(!key_up), 0) == 0
        {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        (xlib.XFlush)(display);
        Ok(())
    })
}

#[cfg(target_os = "linux")]
fn x11_key_name(value: u16) -> Option<String> {
    let name = match value {
        0x0D => "Return".to_string(),
        0x09 => "Tab".to_string(),
        0x1B => "Escape".to_string(),
        0x08 => "BackSpace".to_string(),
        0x2E => "Delete".to_string(),
        0x2D => "Insert".to_string(),
        0x24 => "Home".to_string(),
        0x23 => "End".to_string(),
        0x21 => "Page_Up".to_string(),
        0x22 => "Page_Down".to_string(),
        0x25 => "Left".to_string(),
        0x27 => "Right".to_string(),
        0x26 => "Up".to_string(),
        0x28 => "Down".to_string(),
        0x20 => "space".to_string(),
        0x5B => "Super_L".to_string(),
        value if (0x70..=0x87).contains(&value) => format!("F{}", value - 0x70 + 1),
        value if matches!(value, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A) => {
            char::from_u32(u32::from(value)).map(|ch| ch.to_string())?
        }
        _ => return None,
    };
    Some(name)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn send_key_event(key: VkCode, _key_up: bool) -> Result<()> {
    let _ = match key {
        VkCode::Raw(value) => value,
        VkCode::Control | VkCode::Shift | VkCode::Alt | VkCode::LWin => 0,
    };
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

#[cfg(windows)]
fn is_extended_key(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_UP,
    };
    matches!(
        vk,
        VK_INSERT
            | VK_DELETE
            | VK_HOME
            | VK_END
            | VK_PRIOR
            | VK_NEXT
            | VK_LEFT
            | VK_RIGHT
            | VK_UP
            | VK_DOWN
    )
}

#[cfg(windows)]
fn set_cursor_pos(x: i32, y: i32) -> Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let ok = unsafe { SetCursorPos(x, y) };
    if ok == 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_cursor_pos(x: i32, y: i32) -> Result<()> {
    with_xtest(|xlib, xtest, display, screen| unsafe {
        if (xtest.XTestFakeMotionEvent)(display, screen, x, y, 0) == 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        (xlib.XFlush)(display);
        Ok(())
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn set_cursor_pos(_x: i32, _y: i32) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

#[cfg(windows)]
fn cursor_pos() -> Option<(i32, i32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut point) };
    if ok == 0 {
        return None;
    }
    Some((point.x, point.y))
}

#[cfg(target_os = "linux")]
fn cursor_pos() -> Option<(i32, i32)> {
    with_x11(|xlib, display, screen| unsafe {
        let root = (xlib.XRootWindow)(display, screen);
        let mut root_return = 0;
        let mut child_return = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;
        if (xlib.XQueryPointer)(
            display,
            root,
            &mut root_return,
            &mut child_return,
            &mut root_x,
            &mut root_y,
            &mut win_x,
            &mut win_y,
            &mut mask,
        ) == 0
        {
            return Err(anyhow!("X11 cursor unavailable"));
        }
        Ok((root_x, root_y))
    })
    .ok()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn cursor_pos() -> Option<(i32, i32)> {
    None
}

enum MouseEvent {
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
}

#[cfg(windows)]
fn send_mouse_event(event: MouseEvent) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEINPUT,
    };
    let flags = match event {
        MouseEvent::LeftDown => MOUSEEVENTF_LEFTDOWN,
        MouseEvent::LeftUp => MOUSEEVENTF_LEFTUP,
        MouseEvent::RightDown => MOUSEEVENTF_RIGHTDOWN,
        MouseEvent::RightUp => MOUSEEVENTF_RIGHTUP,
        MouseEvent::MiddleDown => MOUSEEVENTF_MIDDLEDOWN,
        MouseEvent::MiddleUp => MOUSEEVENTF_MIDDLEUP,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_mouse_event(event: MouseEvent) -> Result<()> {
    let button = match event {
        MouseEvent::LeftDown | MouseEvent::LeftUp => 1,
        MouseEvent::MiddleDown | MouseEvent::MiddleUp => 2,
        MouseEvent::RightDown | MouseEvent::RightUp => 3,
    };
    let press = matches!(
        event,
        MouseEvent::LeftDown | MouseEvent::MiddleDown | MouseEvent::RightDown
    );
    with_xtest(|xlib, xtest, display, _| unsafe {
        if (xtest.XTestFakeButtonEvent)(display, button, i32::from(press), 0) == 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_control.capture_failed"
            )));
        }
        (xlib.XFlush)(display);
        Ok(())
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn send_mouse_event(_event: MouseEvent) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

#[cfg(windows)]
fn send_mouse_wheel(steps: i32) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    };
    let delta = steps * 120;
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err(anyhow!(crate::i18n::t(
            "tool.desktop_control.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn send_mouse_wheel(steps: i32) -> Result<()> {
    if steps == 0 {
        return Ok(());
    }
    let button = if steps > 0 { 4 } else { 5 };
    with_xtest(|xlib, xtest, display, _| unsafe {
        for _ in 0..steps.unsigned_abs() {
            if (xtest.XTestFakeButtonEvent)(display, button, 1, 0) == 0
                || (xtest.XTestFakeButtonEvent)(display, button, 0, 0) == 0
            {
                return Err(anyhow!(crate::i18n::t(
                    "tool.desktop_control.capture_failed"
                )));
            }
        }
        (xlib.XFlush)(display);
        Ok(())
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn send_mouse_wheel(_steps: i32) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_control.unsupported_platform"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use uuid::Uuid;

    fn write_test_png(path: &Path, rgba: [u8; 4]) {
        let image = ImageBuffer::<Rgba<u8>, _>::from_pixel(2, 2, Rgba(rgba));
        image.save(path).expect("save test png");
    }

    fn signature_of(value: u8) -> FrameSignature {
        FrameSignature {
            cols: 2,
            rows: 1,
            cells: vec![Some([value, value, value]), Some([value, value, value])],
        }
    }

    #[test]
    fn desktop_stability_waits_through_delayed_visual_change() {
        let mut tracker = FrameStabilityTracker::default();
        assert!(!tracker.observe(&signature_of(0), 320));
        assert!(!tracker.observe(&signature_of(0), 440));
        assert!(!tracker.observe(&signature_of(200), 560));
        assert!(!tracker.observe(&signature_of(200), 680));
        assert!(!tracker.observe(&signature_of(200), 800));
        assert!(tracker.observe(&signature_of(200), 920));
    }

    #[test]
    fn desktop_stability_reports_continuous_animation_as_unsettled() {
        let mut tracker = FrameStabilityTracker::default();
        for probe in 0..=MAX_OBSERVATION_PROBES {
            assert!(!tracker.observe(
                &signature_of(if probe % 2 == 0 { 0 } else { 200 }),
                MIN_OBSERVATION_SETTLE_MS + probe as u64 * OBSERVATION_PROBE_INTERVAL_MS,
            ));
        }
    }

    #[test]
    fn desktop_stability_requires_a_full_quiet_window() {
        let mut tracker = FrameStabilityTracker::default();
        assert!(!tracker.observe(&signature_of(200), 0));
        assert!(!tracker.observe(&signature_of(200), OBSERVATION_QUIET_WINDOW_MS - 1));
        assert!(tracker.observe(&signature_of(200), OBSERVATION_QUIET_WINDOW_MS));
    }

    /// 光标闪烁、秒级时钟、旋转指示器都会让整帧哈希失效，
    /// 于是每次动作都被判定成「画面仍在变化」。少量格子变化必须容忍。
    #[test]
    fn desktop_stability_tolerates_a_blinking_caret() {
        let width = 320usize;
        let height = 180usize;
        let mut frame = vec![0u8; width * height * 4];
        let mut tracker = FrameStabilityTracker::default();
        let base = frame_signature(&frame, width, height, &[]);
        assert!(!tracker.observe(&base, 0));

        // 每帧翻转一个格子（约 1/180 区域内的一小块），模拟闪烁光标。
        for (probe, elapsed) in [(1u64, 120u64), (2, 240), (3, 360)] {
            let offset = (20 * width + 20) * 4;
            let on = probe % 2 == 1;
            frame[offset..offset + 4].copy_from_slice(if on {
                &[255, 255, 255, 255]
            } else {
                &[0, 0, 0, 255]
            });
            let signature = frame_signature(&frame, width, height, &[]);
            let settled = tracker.observe(&signature, elapsed);
            assert_eq!(settled, elapsed >= OBSERVATION_QUIET_WINDOW_MS);
        }
    }

    #[test]
    fn desktop_observation_probe_uses_only_remaining_wall_clock_budget() {
        assert_eq!(observation_probe_timeout_ms(320, 5_000), Some(1_560));
        assert_eq!(observation_probe_timeout_ms(1_800, 5_000), Some(80));
        assert_eq!(observation_probe_timeout_ms(1_880, 5_000), None);
        assert_eq!(observation_probe_timeout_ms(5_000, 5_000), None);
        assert_eq!(observation_probe_timeout_ms(320, 200), Some(200));
    }

    #[tokio::test]
    async fn build_followup_user_message_includes_confirmed_current_frame_only() {
        let temp_dir = std::env::temp_dir().join(format!(
            "wunder-desktop-followup-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let current_path = temp_dir.join("current.png");
        write_test_png(&current_path, [0, 255, 0, 255]);

        let payload = json!({
            "screenshot_path": current_path.to_string_lossy().to_string(),
            "followup_prompt": "check desktop"
        });
        let message = build_followup_user_message(&payload)
            .await
            .expect("build followup")
            .expect("message");
        let content = message
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");
        assert_eq!(content.len(), 2);
        assert_eq!(
            content[0]
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "check desktop"
        );
        assert!(content[1]
            .get("image_url")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .starts_with("data:image/png;base64,"));

        let _ = std::fs::remove_file(current_path);
        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn parse_desktop_control_args_rejects_cross_action_fields() {
        let payload = parse_desktop_control_args(&json!({
            "bbox": [10, 20, 30, 40],
            "action": "left_click",
            "text": "unexpected"
        }));
        assert!(payload.is_err());
    }

    #[test]
    fn parse_desktop_control_args_accepts_action_specific_fields() {
        let payload = parse_desktop_control_args(&json!({
            "bbox": [10, 20, 30, 40],
            "action": "type_text",
            "text": "hello"
        }))
        .expect("parse desktop controller args");
        assert_eq!(payload.action_raw, "type_text");
        assert_eq!(payload.text.as_deref(), Some("hello"));
    }

    #[test]
    fn parse_desktop_control_args_recovers_stringified_bbox_array() {
        let payload = parse_desktop_control_args(&json!({
            "bbox": "[50, 75]",
            "action": "left_click"
        }))
        .expect("recover serialized bbox array");
        assert_eq!(payload.bbox.expect("bbox").center(), (50, 75));
    }

    #[test]
    fn parse_desktop_control_args_recovers_stringified_drag_destination() {
        let payload = parse_desktop_control_args(&json!({
            "bbox": "[10, 20]",
            "to_bbox": "[80, 90]",
            "duration_ms": 100,
            "action": "drag_drop"
        }))
        .expect("recover serialized drag coordinates");
        assert_eq!(payload.bbox.expect("bbox").center(), (10, 20));
        assert_eq!(payload.to_bbox.expect("destination").center(), (80, 90));
    }

    #[test]
    fn parse_desktop_control_args_rejects_non_array_bbox_text() {
        let payload = parse_desktop_control_args(&json!({
            "bbox": "50, 75",
            "action": "left_click"
        }));
        assert!(payload.is_err());
    }

    #[test]
    fn frame_signature_ignores_agent_window_repaint() {
        let width = 64usize;
        let height = 64usize;
        let masks = [(0usize, 0usize, 32usize, 32usize)];
        let mut frame_a = vec![0u8; width * height * 4];
        let mut frame_b = frame_a.clone();
        for y in 0..32 {
            for x in 0..32 {
                let offset = (y * width + x) * 4;
                frame_a[offset..offset + 4].copy_from_slice(&[255, 0, 0, 255]);
                frame_b[offset..offset + 4].copy_from_slice(&[0, 255, 0, 255]);
            }
        }
        let a = frame_signature(&frame_a, width, height, &masks);
        let b = frame_signature(&frame_b, width, height, &masks);
        assert_eq!(a.changed_ratio(&b), 0.0);

        // 遮罩之外的真实重绘必须被检出（整格变化，而不是抗锯齿级别的单像素）。
        for y in 32..48 {
            for x in 32..48 {
                let offset = (y * width + x) * 4;
                frame_b[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let b = frame_signature(&frame_b, width, height, &masks);
        assert!(a.changed_ratio(&b) > FRAME_STABLE_CHANGED_RATIO);
    }

    #[test]
    fn frame_signature_never_calls_a_fully_masked_view_settled() {
        let width = 32usize;
        let height = 32usize;
        let masks = [(0usize, 0usize, 32usize, 32usize)];
        let a = frame_signature(&vec![0u8; width * height * 4], width, height, &masks);
        let b = frame_signature(&vec![255u8; width * height * 4], width, height, &masks);
        assert_eq!(a.changed_ratio(&b), 1.0);
    }

    #[test]
    fn pixel_rect_maps_screen_rect_to_normalized_mask() {
        let rect = PixelRect {
            x: 100,
            y: 50,
            width: 400,
            height: 200,
        };
        assert_eq!(rect.normalized(1000, 500, 100, 50), Some((8, 3, 52, 27)));
    }

    /// 越界坐标必须被拒绝，而不是被钳成屏幕边缘的一次误点击；
    /// 该约束与归一化空间的具体尺寸无关。
    #[test]
    fn bbox_out_of_bounds_rejects_coordinates_past_the_current_extent() {
        assert!(!BBox {
            x1: 0,
            y1: 0,
            x2: 1000,
            y2: 563
        }
        .out_of_bounds(1000, 563));
        assert!(BBox {
            x1: 0,
            y1: 0,
            x2: 1000,
            y2: 564
        }
        .out_of_bounds(1000, 563));
        assert!(BBox {
            x1: 1001,
            y1: 0,
            x2: 1001,
            y2: 0
        }
        .out_of_bounds(1000, 563));
        // 非默认尺寸的归一化空间同样适用。
        assert!(BBox {
            x1: 500,
            y1: 700,
            x2: 500,
            y2: 700
        }
        .out_of_bounds(800, 600));
    }

    /// 归一化空间必须保留屏幕长宽比：16:9 桌面不能被压成正方形，
    /// 否则模型要在变形的画面上判断位置，还多付 78% 的像素。
    #[test]
    fn normalized_space_keeps_the_screen_aspect() {
        assert_eq!(fit_normalized_size(1920, 1080, 1000, 1000), (1000, 563));
        assert_eq!(fit_normalized_size(2560, 1440, 1000, 1000), (1000, 563));
        assert_eq!(fit_normalized_size(3840, 1080, 1000, 1000), (1000, 281));
        // 竖屏反过来锁定高度。
        assert_eq!(fit_normalized_size(1080, 1920, 1000, 1000), (563, 1000));
        // 与归一化框同比例时尺寸不变。
        assert_eq!(fit_normalized_size(1280, 1280, 1000, 1000), (1000, 1000));
        // 退化输入不能产生 0 或负数。
        assert_eq!(fit_normalized_size(0, 0, 1000, 1000), (1000, 1000));
        assert_eq!(fit_normalized_size(-8, 600, 1000, 1000), (1000, 1000));
    }

    /// 持久化到用户目录的截图同样要有界，否则一次会话里每个动作都会
    /// 留一张约 150 KB 的副本，越用越多。
    #[test]
    fn prune_dir_frames_bounds_the_persisted_copies() {
        let dir = std::env::temp_dir().join(format!("wunder-dc-prune-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("create dir");
        for index in 0..5 {
            std::fs::write(dir.join(format!("desktop_shot_{index}.png")), b"x").expect("write");
        }
        prune_dir_frames(&dir, 3);
        let remaining = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .count();
        assert_eq!(remaining, 3);
        // 目录里不足上限时不删任何文件。
        prune_dir_frames(&dir, 8);
        let remaining = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .count();
        assert_eq!(remaining, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 生产默认值（config 的 norm_width/norm_height 缺省）用于真机校验。
    const DEFAULT_NORM_WIDTH_FOR_TEST: i32 = 1280;

    /// 真机校验：截图的实际像素尺寸必须等于按屏幕长宽比推导出的归一化尺寸，
    /// 而不是被拉伸成正方形。
    /// 运行：cargo test -p wunder-runtime --lib --features desktop-control -- --ignored
    #[test]
    #[ignore = "requires a live desktop session"]
    fn live_capture_matches_fitted_normalized_size() {
        let (screen_w, screen_h) = screen_size().expect("screen size");
        let (norm_w, norm_h) = fit_normalized_size(
            screen_w,
            screen_h,
            DEFAULT_NORM_WIDTH_FOR_TEST,
            DEFAULT_NORM_WIDTH_FOR_TEST,
        );
        let shot = capture_screenshot_blocking(norm_w, norm_h, Some((screen_w, screen_h)))
            .expect("capture screenshot");
        let decoded = image::open(&shot.path).expect("decode screenshot");
        println!(
            "screen {screen_w}x{screen_h} -> normalized {norm_w}x{norm_h}, png {} bytes",
            shot.size_bytes
        );
        assert_eq!(decoded.width(), norm_w as u32);
        assert_eq!(decoded.height(), norm_h as u32);
        let _ = std::fs::remove_file(&shot.path);
    }

    /// wait 与 press_key 不移动指针，因此不需要坐标；其他动作缺坐标必须报错，
    /// 而不是默认点到屏幕左上角。
    #[test]
    fn coordinate_is_optional_only_for_pointer_free_actions() {
        let wait = parse_desktop_control_args(&json!({"action": "wait", "delay_ms": 200}))
            .expect("wait without bbox");
        assert!(wait.bbox.is_none());
        assert_eq!(wait.delay_ms, 200);
        assert!(
            parse_desktop_control_args(&json!({"action": "press_key", "key": "enter"})).is_ok()
        );
        assert!(parse_desktop_control_args(&json!({"action": "left_click"})).is_err());
        // type_text targets the current focus when no bbox is given, so it no
        // longer forces a synthetic click on the text field.
        assert!(parse_desktop_control_args(&json!({"action": "type_text", "text": "hi"})).is_ok());
        assert!(parse_desktop_control_args(
            &json!({"action": "type_text", "bbox": [10, 20, 30, 40], "text": "hi"})
        )
        .is_ok());
        assert!(
            parse_desktop_control_args(&json!({"action": "scroll_down", "scroll_steps": 3}))
                .is_err()
        );
    }

    #[test]
    fn desktop_schema_uses_action_variants() {
        let spec = desktop_control_tool_spec("en-US");
        assert!(spec.input_schema["oneOf"]
            .as_array()
            .is_some_and(|items| items.len() >= 7));
        assert!(spec.input_schema["oneOf"][0]["additionalProperties"] == false);
        let type_text = spec.input_schema["oneOf"]
            .as_array()
            .and_then(|variants| {
                variants
                    .iter()
                    .find(|item| item["properties"]["action"]["const"] == "type_text")
            })
            .expect("type_text variant");
        assert_eq!(
            type_text["properties"]["bbox"]["$ref"],
            json!("#/$defs/bbox")
        );
        assert_eq!(
            type_text["properties"]["text"]["$ref"],
            json!("#/$defs/text")
        );
        assert_eq!(
            type_text["required"],
            json!(["action", "text"]),
            "bbox must stay optional for type_text"
        );
        assert_eq!(spec.input_schema["$defs"]["bbox"]["type"], json!("array"));
    }

    #[test]
    fn text_input_segments_preserve_multiline_text() {
        assert_eq!(
            text_input_segments("a\r\nb\nc\td"),
            vec![
                TextSegment::Char('a'),
                TextSegment::Enter,
                TextSegment::Char('b'),
                TextSegment::Enter,
                TextSegment::Char('c'),
                TextSegment::Tab,
                TextSegment::Char('d'),
            ]
        );
        assert_eq!(
            text_input_segments("第一行\n第二行"),
            vec![
                TextSegment::Char('第'),
                TextSegment::Char('一'),
                TextSegment::Char('行'),
                TextSegment::Enter,
                TextSegment::Char('第'),
                TextSegment::Char('二'),
                TextSegment::Char('行'),
            ]
        );
    }
}
