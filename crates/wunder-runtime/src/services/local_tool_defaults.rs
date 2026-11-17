//! 本地形态共享的工具默认值。
//!
//! 桌面版与命令行跑在同一个引擎、读写同一份 `wunder.yaml` 上，能力面必须一
//! 致：任何一方启动都不该改变另一方看到的工具集。此前这套"本地形态开启哪些
//! 工具、桌面控制归一化下限"的开关写在桌面版的启动流程里，命令行只继承了
//! 一半，直接导致命令行缺桌面控制。默认值集中在这里，两端都只调用这一个函数。

use crate::config::Config;

/// 桌面控制归一化坐标的下限。低于它时截图里的文字难以辨认，模型点击精度
/// 会明显下降，因此配置文件里的更小值一律抬到下限。
pub const DESKTOP_CONTROL_MIN_NORM_WIDTH: i32 = 1000;
pub const DESKTOP_CONTROL_MIN_NORM_HEIGHT: i32 = 1000;

/// 把本地形态（桌面版 / 命令行）的工具开关落到配置上。
///
/// 幂等：重复调用不会叠加条目，也不会覆盖用户调高的归一化尺寸。
pub fn apply_local_tool_defaults(config: &mut Config) {
    ensure_builtin_tool(&mut config.tools.builtin.enabled, "计划面板");
    if cfg!(target_os = "android") {
        // 安卓版 shares the local engine but has no window manager, mouse or
        // keyboard. The Android shell installs the phone-control host; a
        // persisted desktop switch from an older build must not revive the
        // desktop tool on the phone.
        config.tools.desktop_control.enabled = false;
        config
            .tools
            .builtin
            .enabled
            .retain(|name| crate::services::tools::resolve_tool_name(name.trim()) != "桌面控制");
    } else {
        config.tools.desktop_control.enabled = true;
        config.tools.desktop_control.norm_width = config
            .tools
            .desktop_control
            .norm_width
            .max(DESKTOP_CONTROL_MIN_NORM_WIDTH);
        config.tools.desktop_control.norm_height = config
            .tools
            .desktop_control
            .norm_height
            .max(DESKTOP_CONTROL_MIN_NORM_HEIGHT);
        ensure_builtin_tool(&mut config.tools.builtin.enabled, "桌面控制");
    }
    // Browser tools are auto-registered from the active provider's native
    // surface, so the switch is the only thing that has to agree between forms.
    config.tools.browser.enabled = true;
}

fn ensure_builtin_tool(enabled: &mut Vec<String>, required: &str) {
    let required = required.trim();
    if required.is_empty() {
        return;
    }
    let has_required = enabled
        .iter()
        .any(|name| crate::services::tools::resolve_tool_name(name.trim()) == required);
    if !has_required {
        enabled.push(required.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_desktop_control_and_browser_for_local_forms() {
        let mut config = Config::default();
        config.tools.desktop_control.enabled = false;
        config.tools.browser.enabled = false;
        config.tools.desktop_control.norm_width = 320;
        config.tools.desktop_control.norm_height = 240;

        apply_local_tool_defaults(&mut config);

        assert_eq!(
            config.tools.desktop_control.enabled,
            !cfg!(target_os = "android")
        );
        assert!(config.tools.browser.enabled);
        assert_eq!(
            config.tools.desktop_control.norm_width,
            DESKTOP_CONTROL_MIN_NORM_WIDTH
        );
        assert_eq!(
            config.tools.desktop_control.norm_height,
            DESKTOP_CONTROL_MIN_NORM_HEIGHT
        );
        assert!(config
            .tools
            .builtin
            .enabled
            .iter()
            .any(|name| name == "计划面板"));
    }

    #[test]
    fn keeps_user_raised_normalized_size() {
        let mut config = Config::default();
        config.tools.desktop_control.norm_width = 1600;
        config.tools.desktop_control.norm_height = 900;

        apply_local_tool_defaults(&mut config);

        assert_eq!(config.tools.desktop_control.norm_width, 1600);
        assert_eq!(
            config.tools.desktop_control.norm_height,
            DESKTOP_CONTROL_MIN_NORM_HEIGHT
        );
    }

    #[test]
    fn is_idempotent() {
        let mut config = Config::default();
        apply_local_tool_defaults(&mut config);
        let first = config.tools.builtin.enabled.clone();
        apply_local_tool_defaults(&mut config);
        assert_eq!(config.tools.builtin.enabled, first);
    }
}
