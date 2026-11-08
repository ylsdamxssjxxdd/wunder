//! The settings page's app-permission catalog.
//!
//! Java reports the raw system state (`granted` / `denied` / `unsupported`);
//! this module owns the user-facing labels and ordering. The catalog is static
//! and bounded, so the UI tick can rebuild it cheaply after every refresh.

use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PermissionItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub state_text: String,
    pub action_text: String,
    pub granted: bool,
    pub available: bool,
}

struct Meta {
    id: &'static str,
    title: &'static str,
    description: &'static str,
}

const ITEMS: &[Meta] = &[
    Meta {
        id: "notifications",
        title: "通知",
        description: "运行状态、完成提醒与前台服务通知",
    },
    Meta {
        id: "microphone",
        title: "麦克风",
        description: "语音输入与转写",
    },
    Meta {
        id: "overlay",
        title: "悬浮窗",
        description: "悬浮浏览器与桌面伙伴显示在其他应用上层",
    },
    Meta {
        id: "accessibility",
        title: "无障碍服务",
        description: "手机控制、界面读取与屏幕截图",
    },
    Meta {
        id: "battery",
        title: "电池优化",
        description: "允许应用在后台持续执行任务",
    },
    Meta {
        id: "install_packages",
        title: "安装应用",
        description: "在线更新与安装 APK",
    },
    Meta {
        id: "all_files",
        title: "所有文件访问",
        description: "让工作区可以指向手机存储中的任意文件夹",
    },
    Meta {
        id: "screen_capture",
        title: "屏幕采集",
        description: "每次截图前由系统确认（单次授权）",
    },
    Meta {
        id: "notification_access",
        title: "通知读取",
        description: "读取通知列表，并在需要时清除通知",
    },
];

pub fn snapshot() -> Vec<PermissionItem> {
    #[cfg(target_os = "android")]
    let states: Value =
        serde_json::from_str(&crate::android_host::permission_snapshot()).unwrap_or(Value::Null);
    #[cfg(not(target_os = "android"))]
    let states = Value::Null;
    from_states(&states)
}

pub fn summary(items: &[PermissionItem]) -> String {
    let granted = items.iter().filter(|item| item.granted).count();
    format!("已开启 {granted}/{} 项", items.len())
}

pub fn open(id: &str) {
    #[cfg(target_os = "android")]
    crate::android_host::open_permission(id);
    #[cfg(not(target_os = "android"))]
    let _ = id;
}

fn from_states(states: &Value) -> Vec<PermissionItem> {
    ITEMS
        .iter()
        .map(|meta| {
            let state = states
                .get(meta.id)
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let granted = state == "granted";
            let available = state != "unsupported";
            let single_use = meta.id == "screen_capture";
            let state_text = match (state, single_use) {
                ("granted", true) => "已授权（单次）",
                ("denied", true) => "按需授权",
                ("granted", false) => "已开启",
                ("denied", false) => "未开启",
                ("unsupported", _) => "系统不支持",
                _ => "未知",
            };
            let action_text = if !available {
                ""
            } else if single_use {
                "去授权"
            } else if granted {
                "去管理"
            } else {
                "去开启"
            };
            PermissionItem {
                id: meta.id.to_string(),
                title: meta.title.to_string(),
                description: meta.description.to_string(),
                state_text: state_text.to_string(),
                action_text: action_text.to_string(),
                granted,
                available,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn states_map_to_labels_and_actions() {
        let items = from_states(&json!({
            "notifications": "granted",
            "microphone": "denied",
            "overlay": "granted",
            "accessibility": "denied",
            "battery": "unsupported",
            "install_packages": "denied"
        }));
        assert_eq!(items.len(), 9);
        assert!(items[0].granted);
        assert_eq!(items[0].state_text, "已开启");
        assert_eq!(items[0].action_text, "去管理");
        assert_eq!(items[1].state_text, "未开启");
        assert_eq!(items[1].action_text, "去开启");
        assert!(!items[4].available);
        assert_eq!(items[4].state_text, "系统不支持");
        assert_eq!(items[4].action_text, "");
        assert_eq!(summary(&items), "已开启 2/9 项");
    }

    #[test]
    fn missing_states_stay_unknown_instead_of_failing() {
        let items = from_states(&json!({}));
        assert_eq!(items.len(), 9);
        assert!(items.iter().all(|item| item.state_text == "未知"));
        assert!(items.iter().all(|item| item.available));
    }

    #[test]
    fn screen_capture_is_labelled_as_single_use() {
        let granted = from_states(&json!({"screen_capture": "granted"}));
        let item = granted
            .iter()
            .find(|item| item.id == "screen_capture")
            .unwrap();
        assert_eq!(item.state_text, "已授权（单次）");
        assert_eq!(item.action_text, "去授权");

        let denied = from_states(&json!({"screen_capture": "denied"}));
        let item = denied
            .iter()
            .find(|item| item.id == "screen_capture")
            .unwrap();
        assert_eq!(item.state_text, "按需授权");
        assert_eq!(item.action_text, "去授权");
    }
}
