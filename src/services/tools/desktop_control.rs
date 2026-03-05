use super::ToolContext;
use crate::config::{Config, DesktopControllerConfig};
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
use crate::storage::USER_PRIVATE_CONTAINER_ID;

pub const TOOL_DESKTOP_CONTROLLER: &str = "桌面控制器";
pub const TOOL_DESKTOP_MONITOR: &str = "桌面监视器";
pub const TOOL_DESKTOP_CONTROLLER_ALIAS: &str = "desktop_controller";
pub const TOOL_DESKTOP_MONITOR_ALIAS: &str = "desktop_monitor";
pub const TOOL_DESKTOP_CONTROLLER_ALIAS_SHORT: &str = "controller";
pub const TOOL_DESKTOP_MONITOR_ALIAS_SHORT: &str = "monitor";

const MAX_MONITOR_WAIT_MS: u64 = 30_000;
const MAX_SCREENSHOT_BYTES: u64 = 8 * 1024 * 1024;

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
    Delay,
    MoveMouse,
    DragDrop,
}

impl DesktopAction {
    fn from_raw(raw: &str) -> Option<Self> {
        let mut cleaned = raw.trim().to_lowercase();
        cleaned = cleaned.replace('-', "_");
        cleaned.retain(|ch| !ch.is_whitespace());
        match cleaned.as_str() {
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
            "press_key" | "keyboard" => Some(Self::PressKey),
            "type_text" | "send_text" => Some(Self::TypeText),
            "delay" | "sleep" => Some(Self::Delay),
            "move_mouse" | "move" => Some(Self::MoveMouse),
            "drag_drop" | "drag" => Some(Self::DragDrop),
            _ => None,
        }
    }
}

struct DesktopControllerArgs {
    bbox: BBox,
    action: DesktopAction,
    action_raw: String,
    description: String,
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
    screen_width: i32,
    screen_height: i32,
    size_bytes: usize,
}

pub fn is_desktop_controller_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_DESKTOP_CONTROLLER {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_DESKTOP_CONTROLLER_ALIAS | TOOL_DESKTOP_CONTROLLER_ALIAS_SHORT
    )
}

pub fn is_desktop_monitor_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_DESKTOP_MONITOR {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_DESKTOP_MONITOR_ALIAS | TOOL_DESKTOP_MONITOR_ALIAS_SHORT
    )
}

pub fn is_desktop_control_tool_name(name: &str) -> bool {
    is_desktop_controller_tool_name(name) || is_desktop_monitor_tool_name(name)
}

pub fn desktop_tools_enabled(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop")
        && config.tools.desktop_controller.enabled
}

pub async fn tool_desktop_controller(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    ensure_desktop_enabled(context.config)?;
    let payload = parse_desktop_controller_args(args)?;
    let started_at = Instant::now();
    let config = context.config.tools.desktop_controller.clone();

    let norm_width = config.norm_width.max(1);
    let norm_height = config.norm_height.max(1);

    let (screen_width, screen_height) = screen_size()?;
    let screen_max_x = (screen_width - 1).max(0);
    let screen_max_y = (screen_height - 1).max(0);

    let (cx_norm_raw, cy_norm_raw) = payload.bbox.center();
    let cx_norm = cx_norm_raw.clamp(0, norm_width);
    let cy_norm = cy_norm_raw.clamp(0, norm_height);
    let cx = map_coord(cx_norm, norm_width, screen_max_x);
    let cy = map_coord(cy_norm, norm_height, screen_max_y);

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
                .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.key_required")))?;
            send_key_sequence(key)?;
        }
        DesktopAction::TypeText => {
            let text = payload
                .text
                .as_deref()
                .unwrap_or("")
                .to_string();
            if text.trim().is_empty() {
                return Err(anyhow!(crate::i18n::t(
                    "tool.desktop_controller.text_required"
                )));
            }
            mouse_click(MouseButton::Left, cx, cy)?;
            send_unicode_text(&text)?;
        }
        DesktopAction::Delay => {
            if payload.delay_ms > 0 {
                sleep(Duration::from_millis(payload.delay_ms)).await;
            }
        }
        DesktopAction::MoveMouse => {
            smooth_move(cx, cy, payload.duration_ms).await?;
        }
        DesktopAction::DragDrop => {
            let to_bbox = payload
                .to_bbox
                .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.to_bbox_required")))?;
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

    let screenshot = capture_screenshot(&config).await?;
    persist_screenshot_to_user_container(context, &screenshot).await;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let prompt = build_followup_prompt(
        crate::i18n::t("tool.desktop_controller.followup_prompt"),
        screenshot.norm_width,
        screenshot.norm_height,
    );
    Ok(json!({
        "status": "ok",
        "action": payload.action_raw,
        "description": payload.description,
        "center_norm": [cx_norm, cy_norm],
        "center_screen": [cx, cy],
        "normalized_width": screenshot.norm_width,
        "normalized_height": screenshot.norm_height,
        "screen_width": screenshot.screen_width,
        "screen_height": screenshot.screen_height,
        "screenshot_path": screenshot.path.to_string_lossy().to_string(),
        "screenshot_download_url": screenshot.download_url,
        "screenshot_bytes": screenshot.size_bytes,
        "elapsed_ms": elapsed_ms,
        "followup_prompt": prompt,
    }))
}

pub async fn tool_desktop_monitor(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    ensure_desktop_enabled(context.config)?;
    let wait_ms = parse_monitor_wait_ms(args)?;
    let config = context.config.tools.desktop_controller.clone();
    if wait_ms > 0 {
        sleep(Duration::from_millis(wait_ms)).await;
    }
    let screenshot = capture_screenshot(&config).await?;
    persist_screenshot_to_user_container(context, &screenshot).await;
    let prompt = build_followup_prompt(
        crate::i18n::t("tool.desktop_monitor.followup_prompt"),
        screenshot.norm_width,
        screenshot.norm_height,
    );
    Ok(json!({
        "status": "ok",
        "wait_ms": wait_ms,
        "normalized_width": screenshot.norm_width,
        "normalized_height": screenshot.norm_height,
        "screen_width": screenshot.screen_width,
        "screen_height": screenshot.screen_height,
        "screenshot_path": screenshot.path.to_string_lossy().to_string(),
        "screenshot_download_url": screenshot.download_url,
        "screenshot_bytes": screenshot.size_bytes,
        "followup_prompt": prompt,
        "note": args.get("note"),
    }))
}

pub async fn build_followup_user_message(result_data: &Value) -> Result<Option<Value>> {
    let payload = parse_followup_payload(result_data)?;
    let bytes = tokio::fs::read(&payload.path)
        .await
        .map_err(|_| anyhow!(crate::i18n::t("tool.desktop_controller.capture_failed")))?;
    if bytes.len() as u64 > MAX_SCREENSHOT_BYTES {
        return Err(anyhow!(crate::i18n::t("tool.desktop_controller.capture_too_large")));
    }
    let data_url = format!("data:image/png;base64,{}", STANDARD.encode(bytes));
    Ok(Some(json!({
        "role": "user",
        "content": [
            { "type": "text", "text": payload.prompt },
            { "type": "image_url", "image_url": { "url": data_url } }
        ]
    })))
}

fn parse_followup_payload(result_data: &Value) -> Result<FollowupPayload> {
    let obj = result_data
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.capture_failed")))?;
    let path = obj
        .get("screenshot_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.capture_failed")))?;
    let prompt = obj
        .get("followup_prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| crate::i18n::t("tool.desktop_controller.followup_prompt"));
    Ok(FollowupPayload {
        path: PathBuf::from(path),
        prompt,
    })
}

struct FollowupPayload {
    path: PathBuf,
    prompt: String,
}

fn parse_desktop_controller_args(args: &Value) -> Result<DesktopControllerArgs> {
    let obj = args
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_args")))?;
    let bbox_value = obj
        .get("bbox")
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.bbox_required")))?;
    let bbox = parse_bbox(bbox_value)?;
    let action_raw = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.action_required")))?;
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::t(
            "tool.desktop_controller.description_required"
        )))?
        .to_string();
    let action = DesktopAction::from_raw(action_raw)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.unknown_action")))?;
    let delay_ms = parse_u64(obj.get("delay_ms")).unwrap_or(0);
    let duration_ms = parse_u64(obj.get("duration_ms")).unwrap_or(0);
    let scroll_steps = parse_i32(obj.get("scroll_steps")).unwrap_or(1).max(1);
    let to_bbox = obj.get("to_bbox").and_then(|value| parse_bbox(value).ok());
    Ok(DesktopControllerArgs {
        bbox,
        action,
        action_raw: action_raw.to_string(),
        description,
        key: obj.get("key").and_then(Value::as_str).map(ToString::to_string),
        text: obj.get("text").and_then(Value::as_str).map(ToString::to_string),
        delay_ms,
        duration_ms,
        scroll_steps,
        to_bbox,
    })
}

fn parse_monitor_wait_ms(args: &Value) -> Result<u64> {
    let obj = args
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_monitor.wait_required")))?;
    let raw = obj
        .get("wait_ms")
        .or_else(|| obj.get("wait"))
        .or_else(|| obj.get("delay_ms"));
    let wait_ms = parse_u64(raw)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_monitor.wait_required")))?;
    Ok(wait_ms.min(MAX_MONITOR_WAIT_MS))
}

fn parse_bbox(value: &Value) -> Result<BBox> {
    let arr = value
        .as_array()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
    if arr.len() == 4 {
        let x1 = parse_i32(Some(&arr[0]))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
        let y1 = parse_i32(Some(&arr[1]))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
        let x2 = parse_i32(Some(&arr[2]))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
        let y2 = parse_i32(Some(&arr[3]))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
        return Ok(BBox { x1, y1, x2, y2 });
    }
    if arr.len() == 2 {
        let cx = parse_i32(Some(&arr[0]))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
        let cy = parse_i32(Some(&arr[1]))
            .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.invalid_bbox")))?;
        return Ok(BBox {
            x1: cx,
            y1: cy,
            x2: cx,
            y2: cy,
        });
    }
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.invalid_bbox"
    )))
}

fn parse_i32(value: Option<&Value>) -> Option<i32> {
    let value = value?;
    if let Some(num) = value.as_i64() {
        return Some(num as i32);
    }
    if let Some(num) = value.as_f64() {
        return Some(num.round() as i32);
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| text.parse::<f64>().ok())
        .map(|num| num.round() as i32)
}

fn parse_u64(value: Option<&Value>) -> Option<u64> {
    let value = value?;
    if let Some(num) = value.as_u64() {
        return Some(num);
    }
    if let Some(num) = value.as_i64() {
        if num >= 0 {
            return Some(num as u64);
        }
    }
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .and_then(|text| text.parse::<f64>().ok())
        .filter(|num| num.is_finite() && *num >= 0.0)
        .map(|num| num.round() as u64)
}

fn ensure_desktop_enabled(config: &Config) -> Result<()> {
    if desktop_tools_enabled(config) {
        return Ok(());
    }
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.disabled"
    )))
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

async fn capture_screenshot(config: &DesktopControllerConfig) -> Result<DesktopScreenshot> {
    let norm_width = config.norm_width.max(1);
    let norm_height = config.norm_height.max(1);
    let timeout_ms = config.capture_timeout_ms.max(100);
    let max_frames = config.max_frames.max(1);
    let result = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        tokio::task::spawn_blocking(move || capture_screenshot_blocking(norm_width, norm_height)),
    )
    .await
    .map_err(|_| anyhow!(crate::i18n::t("tool.desktop_controller.capture_timeout")))?;
    let screenshot = result
        .map_err(|err| anyhow!(err.to_string()))?
        .map_err(|err| anyhow!(err.to_string()))?;
    cleanup_old_frames(&screenshot.path, max_frames);
    Ok(screenshot)
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
    let relative = format!("desktop_controller/{safe_session}/{filename}");
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
    if let Some(parent) = dest.parent() {
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
    }
}

fn cleanup_old_frames(path: &Path, max_frames: usize) {
    let Some(dir) = path.parent() else {
        return;
    };
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

fn capture_screenshot_blocking(norm_width: i32, norm_height: i32) -> Result<DesktopScreenshot> {
    let (screen_width, screen_height, rgba) = capture_screen_rgba()?;
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
            "tool.desktop_controller.capture_too_large"
        )));
    }
    let dir = resolve_temp_dir()?.join("desktop_controller");
    std::fs::create_dir_all(&dir)
        .map_err(|err| anyhow!(format!("create temp dir failed: {err}")))?;
    let filename = format!("desktop_shot_{}.png", Uuid::new_v4().simple());
    let path = dir.join(&filename);
    std::fs::write(&path, &png).map_err(|err| anyhow!(format!("write screenshot failed: {err}")))?;
    let download_url = format!("/wunder/temp_dir/download?filename=desktop_controller/{filename}");
    Ok(DesktopScreenshot {
        path,
        download_url,
        norm_width,
        norm_height,
        screen_width,
        screen_height,
        size_bytes: png.len(),
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
    Ok(root.join("temp_dir"))
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
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.capture_failed")))?;
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
            "tool.desktop_controller.capture_failed"
        )));
    }
    Ok((width, height))
}

#[cfg(windows)]
fn screen_metrics() -> Result<(i32, i32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    Ok((width, height))
}

#[cfg(not(windows))]
fn screen_metrics() -> Result<(i32, i32)> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.unsupported_platform"
    )))
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
            "tool.desktop_controller.capture_failed"
        )));
    }

    unsafe {
        let screen_dc: HDC = GetDC(HWND::default());
        if screen_dc == 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_controller.capture_failed"
            )));
        }
        let mem_dc: HDC = CreateCompatibleDC(screen_dc);
        if mem_dc == 0 {
            ReleaseDC(HWND::default(), screen_dc);
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_controller.capture_failed"
            )));
        }
        let bmp: HBITMAP = CreateCompatibleBitmap(screen_dc, width, height);
        if bmp == 0 {
            DeleteDC(mem_dc);
            ReleaseDC(HWND::default(), screen_dc);
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_controller.capture_failed"
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
                "tool.desktop_controller.capture_failed"
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
                "tool.desktop_controller.capture_failed"
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

#[cfg(not(windows))]
fn capture_screen_rgba() -> Result<(i32, i32, Vec<u8>)> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.unsupported_platform"
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
    let parts: Vec<&str> = keys.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err(anyhow!(crate::i18n::t("tool.desktop_controller.key_required")));
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
        return Err(anyhow!(crate::i18n::t("tool.desktop_controller.key_required")));
    }
    for modifier in modifiers {
        key_down(modifier)?;
        key_up(modifier)?;
    }
    Ok(())
}

fn send_unicode_text(text: &str) -> Result<()> {
    for ch in text.chars() {
        send_unicode_char(ch)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Ok(())
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
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.desktop_controller.key_required")))?;
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

fn send_unicode_char(ch: char) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        };
        let mut inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: ch as u16,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: ch as u16,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_mut_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent == 0 {
            return Err(anyhow!(crate::i18n::t(
                "tool.desktop_controller.capture_failed"
            )));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = ch;
        Err(anyhow!(crate::i18n::t(
            "tool.desktop_controller.unsupported_platform"
        )))
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
            "tool.desktop_controller.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_key_event(key: VkCode, _key_up: bool) -> Result<()> {
    let _ = match key {
        VkCode::Raw(value) => value,
        VkCode::Control | VkCode::Shift | VkCode::Alt | VkCode::LWin => 0,
    };
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.unsupported_platform"
    )))
}

#[cfg(windows)]
fn is_extended_key(vk: u16) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        VK_DELETE, VK_DOWN, VK_END, VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_PRIOR, VK_RIGHT,
        VK_UP,
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
            "tool.desktop_controller.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_cursor_pos(_x: i32, _y: i32) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.unsupported_platform"
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

#[cfg(not(windows))]
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
        SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
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
            "tool.desktop_controller.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_mouse_event(_event: MouseEvent) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.unsupported_platform"
    )))
}

#[cfg(windows)]
fn send_mouse_wheel(steps: i32) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_MOUSE, MOUSEINPUT, MOUSEEVENTF_WHEEL,
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
            "tool.desktop_controller.capture_failed"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn send_mouse_wheel(_steps: i32) -> Result<()> {
    Err(anyhow!(crate::i18n::t(
        "tool.desktop_controller.unsupported_platform"
    )))
}
