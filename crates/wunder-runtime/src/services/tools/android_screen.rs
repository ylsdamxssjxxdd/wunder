//! Android screen capture: the MediaProjection counterpart of phone control.
//!
//! The runtime owns argument validation, result shaping and localization. The
//! device side (consent dialog, foreground service, virtual display, PNG file)
//! is provided by a host installed by the Android shell, so 桌面版/命令行/服务端
//! never advertise this tool and stay platform-neutral.
//!
//! The tool captures a single frame. Continuous recording, touch injection and
//! shell-level capture (Shizuku/Root) are deliberately out of scope.

use super::{build_model_tool_success_with_hint, ToolContext};
use crate::schemas::ToolSpec;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TOOL_ANDROID_SCREEN: &str = "安卓屏幕";
pub const TOOL_ANDROID_SCREEN_ALIAS: &str = "android_screen";
pub const TOOL_ANDROID_SCREEN_ALIAS_SHORT: &str = "screen_capture";

const HOST_TIMEOUT: Duration = Duration::from_secs(20);

/// One device-side executor installed by the Android shell. Calls arrive on a
/// blocking worker; implementations may block while the consent dialog opens
/// or a frame is mirrored and encoded.
pub trait AndroidScreenHost: Send + Sync {
    /// `args` currently carries no fields; the action alone selects behavior.
    /// The returned object follows the contract parsed by
    /// [`execute_with_host`].
    fn execute(&self, action: &str, args: &Value) -> Result<Value>;
}

static HOST: std::sync::RwLock<Option<Arc<dyn AndroidScreenHost>>> = std::sync::RwLock::new(None);

/// The shell installs the host at startup, before the first turn freezes its
/// tool surface. A later install replaces the earlier one so a recreated
/// activity cannot leave a stale bridge behind.
pub fn install_android_screen_host(host: Arc<dyn AndroidScreenHost>) {
    let mut slot = HOST.write().unwrap_or_else(|error| error.into_inner());
    *slot = Some(host);
}

/// The tool is offered only when a device executor is installed.
pub fn android_screen_tools_enabled() -> bool {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .is_some()
}

fn android_screen_host() -> Option<Arc<dyn AndroidScreenHost>> {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn is_android_screen_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if matches!(cleaned, TOOL_ANDROID_SCREEN | "安卓截屏" | "屏幕采集") {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_ANDROID_SCREEN_ALIAS | TOOL_ANDROID_SCREEN_ALIAS_SHORT
    )
}

pub fn android_screen_tool_spec(language: &str) -> ToolSpec {
    let t = |key: &str| crate::i18n::t_in_language(key, language);
    ToolSpec {
        name: TOOL_ANDROID_SCREEN.to_string(),
        title: None,
        description: t("tool.spec.android_screen.description"),
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["capture", "status", "request_permission"],
                    "description": t("tool.spec.android_screen.args.action")
                }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScreenAction {
    Capture,
    Status,
    RequestPermission,
}

impl ScreenAction {
    fn from_raw(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "capture" => Some(Self::Capture),
            "status" => Some(Self::Status),
            "request_permission" => Some(Self::RequestPermission),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Status => "status",
            Self::RequestPermission => "request_permission",
        }
    }
}

fn parse_screen_args(args: &Value) -> Result<ScreenAction> {
    let obj = args
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_screen.invalid_args")))?;
    let action_raw = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_screen.action_required")))?;
    ScreenAction::from_raw(action_raw)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_screen.unknown_action")))
}

pub async fn tool_android_screen(_context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let action = parse_screen_args(args)?;
    let host = android_screen_host()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_screen.disabled")))?;
    execute_with_host(host, action).await
}

async fn execute_with_host(
    host: Arc<dyn AndroidScreenHost>,
    action: ScreenAction,
) -> Result<Value> {
    let started_at = Instant::now();
    let reply = crate::core::blocking::run_external_with_timeout(
        "android_screen",
        HOST_TIMEOUT,
        move || host.execute(action.as_str(), &json!({})),
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

    let path = reply
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let width = reply.get("width").and_then(Value::as_i64).unwrap_or(0);
    let height = reply.get("height").and_then(Value::as_i64).unwrap_or(0);
    let authorized = reply
        .get("authorized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let projecting = reply
        .get("projecting")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let service_running = reply
        .get("service_running")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let screen_width = reply
        .get("screen_width")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let screen_height = reply
        .get("screen_height")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    let mut data = json!({
        "action": action.as_str(),
        "action_applied": ok,
        "state": if ok { completed_state(action) } else { "failed" },
        "message": message.clone().unwrap_or_default(),
        "elapsed_ms": started_at.elapsed().as_millis() as u64,
    });

    let mut hint = None;
    match (ok, action) {
        (true, ScreenAction::Capture) => {
            let Some(path) = path.as_deref() else {
                // A successful capture without a file is a device bug; report
                // it as a failed observation instead of a misleading success.
                data["action_applied"] = json!(false);
                data["state"] = json!("failed");
                data["code"] = json!("capture_failed");
                data["message"] = json!(crate::i18n::t("tool.android_screen.capture_failed"));
                return Ok(build_model_tool_success_with_hint(
                    TOOL_ANDROID_SCREEN_ALIAS,
                    "failed",
                    crate::i18n::t("tool.android_screen.action_failed"),
                    data,
                    None,
                ));
            };
            let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            data["path"] = json!(path);
            data["width"] = json!(width);
            data["height"] = json!(height);
            data["screenshot_bytes"] = json!(bytes);
            hint = Some(crate::i18n::t("tool.android_screen.next_step_hint"));
        }
        (true, ScreenAction::Status) => {
            data["authorized"] = json!(authorized);
            data["projecting"] = json!(projecting);
            data["service_running"] = json!(service_running);
            data["screen_width"] = json!(screen_width);
            data["screen_height"] = json!(screen_height);
        }
        (true, ScreenAction::RequestPermission) => {
            data["requested"] = json!(true);
        }
        (false, _) => {
            let fallback = crate::i18n::t("tool.android_screen.action_failed");
            let localized = localize_screen_message(code, message.as_deref(), &fallback);
            data["message"] = json!(localized);
            if let Some(code) = code {
                data["code"] = json!(code);
            }
        }
    }

    let summary = if ok {
        screen_summary(action, authorized, width, height)
    } else {
        data["message"]
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| crate::i18n::t("tool.android_screen.action_failed"))
    };
    Ok(build_model_tool_success_with_hint(
        TOOL_ANDROID_SCREEN_ALIAS,
        if ok {
            completed_state(action)
        } else {
            "failed"
        },
        summary,
        data,
        hint,
    ))
}

fn completed_state(action: ScreenAction) -> &'static str {
    match action {
        ScreenAction::Capture | ScreenAction::Status => "completed",
        ScreenAction::RequestPermission => "pending",
    }
}

fn screen_summary(action: ScreenAction, authorized: bool, width: i64, height: i64) -> String {
    match action {
        ScreenAction::Capture => crate::i18n::t_with_params(
            "tool.android_screen.summary_capture",
            &HashMap::from([
                ("width".to_string(), width.to_string()),
                ("height".to_string(), height.to_string()),
            ]),
        ),
        ScreenAction::Status => {
            let key = if authorized {
                "tool.android_screen.summary_status_on"
            } else {
                "tool.android_screen.summary_status_off"
            };
            crate::i18n::t(key)
        }
        ScreenAction::RequestPermission => crate::i18n::t("tool.android_screen.summary_permission"),
    }
}

/// Device-side failures use stable codes so the runtime can localize them;
/// unknown codes fall back to the host message, then to `fallback`.
fn localize_screen_message(code: Option<&str>, message: Option<&str>, fallback: &str) -> String {
    let key = match code {
        Some("permission_required") | Some("projection_stopped") => {
            Some("tool.android_screen.permission_required")
        }
        Some("service_unavailable") => Some("tool.android_screen.service_unavailable"),
        Some("activity_unavailable") => Some("tool.android_screen.activity_unavailable"),
        Some("capture_failed") | Some("capture_timeout") => {
            Some("tool.android_screen.capture_failed")
        }
        _ => None,
    };
    if let Some(key) = key {
        let localized = crate::i18n::t(key);
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

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost {
        seen: std::sync::Mutex<Vec<String>>,
        reply: Value,
    }

    impl AndroidScreenHost for FakeHost {
        fn execute(&self, action: &str, _args: &Value) -> Result<Value> {
            self.seen.lock().unwrap().push(action.to_string());
            Ok(self.reply.clone())
        }
    }

    fn temp_png() -> tempfile::NamedTempFile {
        let file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        std::fs::write(file.path(), b"png-bytes").unwrap();
        file
    }

    #[test]
    fn missing_action_is_rejected() {
        assert!(parse_screen_args(&json!({})).is_err());
        assert_eq!(
            parse_screen_args(&json!({"action": "capture"})).unwrap(),
            ScreenAction::Capture
        );
        assert!(parse_screen_args(&json!({"action": "record"})).is_err());
    }

    #[tokio::test]
    async fn capture_reports_path_and_size() {
        let file = temp_png();
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: json!({
                "ok": true,
                "path": file.path().to_string_lossy(),
                "width": 1080,
                "height": 2400
            }),
        });
        let result = execute_with_host(host.clone(), ScreenAction::Capture)
            .await
            .unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["data"]["state"], json!("completed"));
        assert_eq!(result["data"]["width"], json!(1080));
        assert_eq!(result["data"]["screenshot_bytes"], json!(9));
        assert!(result["next_step_hint"].is_string());
        assert_eq!(host.seen.lock().unwrap()[0], "capture");
    }

    #[tokio::test]
    async fn permission_failure_is_reported_without_erroring() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: json!({
                "ok": false,
                "code": "permission_required",
                "message": "Screen capture is not authorized."
            }),
        });
        let result = execute_with_host(host, ScreenAction::Capture)
            .await
            .unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["data"]["state"], json!("failed"));
        assert_eq!(result["data"]["action_applied"], json!(false));
        assert_eq!(result["data"]["code"], json!("permission_required"));
    }

    #[tokio::test]
    async fn status_reports_authorization() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: json!({
                "ok": true,
                "authorized": true,
                "projecting": false,
                "service_running": true,
                "screen_width": 1080,
                "screen_height": 2400
            }),
        });
        let result = execute_with_host(host, ScreenAction::Status).await.unwrap();
        assert_eq!(result["data"]["authorized"], json!(true));
        assert_eq!(result["data"]["service_running"], json!(true));
        assert_eq!(result["data"]["screen_height"], json!(2400));
    }

    #[test]
    fn aliases_resolve_to_android_screen() {
        assert!(is_android_screen_tool_name("安卓屏幕"));
        assert!(is_android_screen_tool_name("android_screen"));
        assert!(is_android_screen_tool_name("screen_capture"));
        assert!(!is_android_screen_tool_name("手机控制"));
    }
}
