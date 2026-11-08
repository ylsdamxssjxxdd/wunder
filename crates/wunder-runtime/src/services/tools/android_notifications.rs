//! Android notifications: a bounded read/clear surface over the notification
//! listener that the user enables in system settings.
//!
//! The runtime validates arguments, bounds list sizes and localizes failures;
//! the device side keeps a capped in-memory queue with plain string fields, so
//! no `PendingIntent` or remote view ever crosses the JNI boundary. 桌面版,
//! 命令行 and 服务端 never install a host, so the tool stays Android-only.

use super::{build_model_tool_success, ToolContext};
use crate::schemas::ToolSpec;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TOOL_ANDROID_NOTIFICATIONS: &str = "安卓通知";
pub const TOOL_ANDROID_NOTIFICATIONS_ALIAS: &str = "android_notifications";
pub const TOOL_ANDROID_NOTIFICATIONS_ALIAS_SHORT: &str = "notification_listener";

const DEFAULT_LIMIT: u32 = 20;
const MAX_LIMIT: u32 = 50;
const MAX_KEY_CHARS: usize = 512;
const MAX_PACKAGE_CHARS: usize = 200;
const MAX_TITLE_CHARS: usize = 200;
const MAX_TEXT_CHARS: usize = 300;
const HOST_TIMEOUT: Duration = Duration::from_secs(15);

/// One device-side executor installed by the Android shell.
pub trait AndroidNotificationsHost: Send + Sync {
    /// `args` carries `limit` for list and `key` for dismiss. The returned
    /// object follows the contract parsed by [`execute_with_host`].
    fn execute(&self, action: &str, args: &Value) -> Result<Value>;
}

static HOST: std::sync::RwLock<Option<Arc<dyn AndroidNotificationsHost>>> =
    std::sync::RwLock::new(None);

/// Installed at startup before the first turn freezes its tool surface.
pub fn install_android_notifications_host(host: Arc<dyn AndroidNotificationsHost>) {
    let mut slot = HOST.write().unwrap_or_else(|error| error.into_inner());
    *slot = Some(host);
}

/// The tool is offered only when a device executor is installed.
pub fn android_notifications_tools_enabled() -> bool {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .is_some()
}

fn android_notifications_host() -> Option<Arc<dyn AndroidNotificationsHost>> {
    HOST.read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn is_android_notifications_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if matches!(
        cleaned,
        TOOL_ANDROID_NOTIFICATIONS | "安卓通知栏" | "通知监听"
    ) {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_ANDROID_NOTIFICATIONS_ALIAS | TOOL_ANDROID_NOTIFICATIONS_ALIAS_SHORT
    )
}

pub fn android_notifications_tool_spec(language: &str) -> ToolSpec {
    let t = |key: &str| crate::i18n::t_in_language(key, language);
    ToolSpec {
        name: TOOL_ANDROID_NOTIFICATIONS.to_string(),
        title: None,
        description: t("tool.spec.android_notifications.description"),
        input_schema: json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "dismiss", "status"],
                    "description": t("tool.spec.android_notifications.args.action")
                },
                "key": {
                    "type": "string",
                    "maxLength": MAX_KEY_CHARS,
                    "description": t("tool.spec.android_notifications.args.key")
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LIMIT,
                    "description": t("tool.spec.android_notifications.args.limit")
                }
            },
            "required": ["action"],
            "additionalProperties": false
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotificationsAction {
    List,
    Dismiss,
    Status,
}

impl NotificationsAction {
    fn from_raw(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "list" => Some(Self::List),
            "dismiss" => Some(Self::Dismiss),
            "status" => Some(Self::Status),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Dismiss => "dismiss",
            Self::Status => "status",
        }
    }
}

#[derive(Debug)]
struct NotificationsArgs {
    action: NotificationsAction,
    limit: u32,
    key: Option<String>,
}

impl NotificationsArgs {
    fn host_args(&self) -> Value {
        match self.action {
            NotificationsAction::List => json!({"limit": self.limit}),
            NotificationsAction::Dismiss => {
                json!({"key": self.key.clone().unwrap_or_default()})
            }
            NotificationsAction::Status => json!({}),
        }
    }
}

fn parse_notifications_args(args: &Value) -> Result<NotificationsArgs> {
    let obj = args
        .as_object()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_notifications.invalid_args")))?;
    let action_raw = obj
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_notifications.action_required")))?;
    let action = NotificationsAction::from_raw(action_raw)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_notifications.unknown_action")))?;
    let limit = obj
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIMIT as u64)
        .clamp(1, MAX_LIMIT as u64) as u32;
    let key = obj
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(MAX_KEY_CHARS).collect::<String>());
    if action == NotificationsAction::Dismiss && key.is_none() {
        return Err(anyhow!(crate::i18n::t(
            "tool.android_notifications.key_required"
        )));
    }
    Ok(NotificationsArgs { action, limit, key })
}

pub async fn tool_android_notifications(_context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload = parse_notifications_args(args)?;
    let host = android_notifications_host()
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.android_notifications.disabled")))?;
    execute_with_host(host, payload).await
}

async fn execute_with_host(
    host: Arc<dyn AndroidNotificationsHost>,
    payload: NotificationsArgs,
) -> Result<Value> {
    let action = payload.action;
    let host_args = payload.host_args();
    let started_at = Instant::now();
    let reply = crate::core::blocking::run_external_with_timeout(
        "android_notifications",
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
    let data = reply.get("data").cloned().unwrap_or(Value::Null);

    let mut output = json!({
        "action": action.as_str(),
        "action_applied": ok,
        "state": if ok { "completed" } else { "failed" },
        "message": message.clone().unwrap_or_default(),
        "elapsed_ms": started_at.elapsed().as_millis() as u64,
    });

    if ok {
        match action {
            NotificationsAction::List => {
                let items = data
                    .get("notifications")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .take(MAX_LIMIT as usize)
                            .map(project_notification)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                output["count"] = json!(items.len());
                output["total"] = json!(data
                    .get("total")
                    .and_then(Value::as_i64)
                    .unwrap_or(items.len() as i64));
                output["notifications"] = json!(items);
            }
            NotificationsAction::Dismiss => {
                output["dismissed"] = json!(true);
                if let Some(key) = payload.key.as_deref() {
                    output["key"] = json!(key);
                }
            }
            NotificationsAction::Status => {
                output["connected"] = json!(data
                    .get("connected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false));
                output["count"] = json!(data.get("count").and_then(Value::as_i64).unwrap_or(0));
            }
        }
    } else {
        let fallback = crate::i18n::t("tool.android_notifications.action_failed");
        output["message"] = json!(localize_notifications_message(
            code,
            message.as_deref(),
            &fallback
        ));
        if let Some(code) = code {
            output["code"] = json!(code);
        }
    }

    let summary = if ok {
        notifications_summary(action, &output)
    } else {
        output["message"]
            .as_str()
            .map(ToString::to_string)
            .unwrap_or_else(|| crate::i18n::t("tool.android_notifications.action_failed"))
    };
    Ok(build_model_tool_success(
        TOOL_ANDROID_NOTIFICATIONS_ALIAS,
        if ok { "completed" } else { "failed" },
        summary,
        output,
    ))
}

fn project_notification(item: &Value) -> Value {
    json!({
        "package": bounded_string(item.get("package"), MAX_PACKAGE_CHARS),
        "title": bounded_string(item.get("title"), MAX_TITLE_CHARS),
        "text": bounded_string(item.get("text"), MAX_TEXT_CHARS),
        "post_time": item.get("post_time").and_then(Value::as_i64).unwrap_or(0),
        "key": bounded_string(item.get("key"), MAX_KEY_CHARS),
        "can_dismiss": item.get("can_dismiss").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn bounded_string(value: Option<&Value>, max_chars: usize) -> String {
    value
        .and_then(Value::as_str)
        .map(|value| value.chars().take(max_chars).collect())
        .unwrap_or_default()
}

fn notifications_summary(action: NotificationsAction, data: &Value) -> String {
    match action {
        NotificationsAction::List => crate::i18n::t_with_params(
            "tool.android_notifications.summary_list",
            &HashMap::from([
                (
                    "count".to_string(),
                    data.get("count")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .to_string(),
                ),
                (
                    "total".to_string(),
                    data.get("total")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .to_string(),
                ),
            ]),
        ),
        NotificationsAction::Dismiss => {
            crate::i18n::t("tool.android_notifications.summary_dismiss")
        }
        NotificationsAction::Status => {
            let key = if data
                .get("connected")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                "tool.android_notifications.summary_status_on"
            } else {
                "tool.android_notifications.summary_status_off"
            };
            crate::i18n::t(key)
        }
    }
}

fn localize_notifications_message(
    code: Option<&str>,
    message: Option<&str>,
    fallback: &str,
) -> String {
    let key = match code {
        Some("notification_access_required") => Some("tool.android_notifications.access_required"),
        Some("missing_key") => Some("tool.android_notifications.key_required"),
        Some("dismiss_failed") => Some("tool.android_notifications.dismiss_failed"),
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
        seen: std::sync::Mutex<Vec<(String, Value)>>,
        reply: Value,
    }

    impl AndroidNotificationsHost for FakeHost {
        fn execute(&self, action: &str, args: &Value) -> Result<Value> {
            self.seen
                .lock()
                .unwrap()
                .push((action.to_string(), args.clone()));
            Ok(self.reply.clone())
        }
    }

    fn reply_list() -> Value {
        json!({
            "ok": true,
            "data": {
                "notifications": [
                    {
                        "package": "com.example.chat",
                        "title": "标题",
                        "text": "正文",
                        "post_time": 1700000000000_i64,
                        "key": "0|com.example.chat|1|null|10",
                        "can_dismiss": true
                    },
                    {
                        "package": "com.example.music",
                        "title": "正在播放",
                        "text": "",
                        "post_time": 1699999999999_i64,
                        "key": "0|com.example.music|2|null|11",
                        "can_dismiss": false
                    }
                ],
                "count": 2,
                "total": 12
            }
        })
    }

    #[tokio::test]
    async fn list_projects_bounded_entries() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: reply_list(),
        });
        let payload = NotificationsArgs {
            action: NotificationsAction::List,
            limit: 5,
            key: None,
        };
        let result = execute_with_host(host.clone(), payload).await.unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["data"]["state"], json!("completed"));
        assert_eq!(result["data"]["count"], json!(2));
        assert_eq!(result["data"]["total"], json!(12));
        assert_eq!(
            result["data"]["notifications"][0]["package"],
            json!("com.example.chat")
        );
        let seen = host.seen.lock().unwrap();
        assert_eq!(seen[0].0, "list");
        assert_eq!(seen[0].1, json!({"limit": 5}));
    }

    #[tokio::test]
    async fn dismiss_requires_a_key_before_the_device_call() {
        assert!(parse_notifications_args(&json!({"action": "dismiss"})).is_err());
        let payload = parse_notifications_args(&json!({"action": "dismiss", "key": "k"}))
            .expect("dismiss args");
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: json!({"ok": true, "data": {"dismissed": true}}),
        });
        let result = execute_with_host(host.clone(), payload).await.unwrap();
        assert_eq!(result["data"]["dismissed"], json!(true));
        assert_eq!(result["data"]["key"], json!("k"));
        assert_eq!(host.seen.lock().unwrap()[0].1, json!({"key": "k"}));
    }

    #[tokio::test]
    async fn missing_access_is_reported_without_erroring() {
        let host = Arc::new(FakeHost {
            seen: Default::default(),
            reply: json!({
                "ok": false,
                "code": "notification_access_required",
                "message": "Notification access is not granted."
            }),
        });
        let payload = NotificationsArgs {
            action: NotificationsAction::List,
            limit: DEFAULT_LIMIT,
            key: None,
        };
        let result = execute_with_host(host, payload).await.unwrap();
        assert_eq!(result["ok"], json!(true));
        assert_eq!(result["data"]["state"], json!("failed"));
        assert_eq!(
            result["data"]["code"],
            json!("notification_access_required")
        );
    }

    #[test]
    fn limit_is_clamped_to_the_supported_window() {
        let high = parse_notifications_args(&json!({"action": "list", "limit": 999})).unwrap();
        assert_eq!(high.limit, MAX_LIMIT);
        let low = parse_notifications_args(&json!({"action": "list", "limit": 0})).unwrap();
        assert_eq!(low.limit, 1);
        let default = parse_notifications_args(&json!({"action": "list"})).unwrap();
        assert_eq!(default.limit, DEFAULT_LIMIT);
    }

    #[test]
    fn aliases_resolve_to_android_notifications() {
        assert!(is_android_notifications_tool_name("安卓通知"));
        assert!(is_android_notifications_tool_name("android_notifications"));
        assert!(is_android_notifications_tool_name("notification_listener"));
        assert!(!is_android_notifications_tool_name("手机控制"));
    }
}
