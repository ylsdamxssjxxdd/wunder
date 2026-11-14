//! One acknowledgement channel for user-visible operations.
//!
//! Every handler writes its progress and result into `MainWindow.status`, but no
//! view renders that property: before this module a successful save, restore or
//! archive left the UI completely silent, and failures only escaped through a
//! fragile Slint string heuristic that missed messages such as `无法…`.
//!
//! Two entry points keep the shell honest:
//!
//! * [`install`] bridges every legacy `status` write to the toast. Slint reports
//!   the write, [`classify`] decides whether it is a result (surface it) or
//!   progress/startup chatter (stay quiet).
//! * [`success`] / [`failure`] are the explicit acknowledgements for
//!   operations whose result no other view shows. New code should prefer these,
//!   because they can also carry an English string next to the Chinese one.
//!
//! The toast itself auto-hides in Slint; this module only owns what to show and
//! which colour it takes (`0` success, `1` information, `2` failure).

use slint::{ComponentHandle, SharedString};

use crate::{MainWindow, ToastState};

pub(crate) const KIND_SUCCESS: i32 = 0;
pub(crate) const KIND_INFO: i32 = 1;
pub(crate) const KIND_ERROR: i32 = 2;

/// Wire the `status` bridge. Called once by the native shell (and by the demo
/// preview) right after the window is created, so even startup-time writes are
/// classified consistently.
pub fn install(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_status_feedback(move |message| {
        let Some(app) = weak.upgrade() else { return };
        if let Some(kind) = classify(&message) {
            notify(&app, message, kind);
        }
    });
}

/// Show `message` as a toast. A toast already showing the same text is left
/// untouched: a retry loop must not restart the banner's hide timer forever.
pub fn notify(app: &MainWindow, message: impl Into<SharedString>, kind: i32) {
    let message = message.into();
    if message.trim().is_empty() {
        return;
    }
    let toast = app.global::<ToastState>();
    if toast.get_message() == message {
        return;
    }
    toast.set_message(message);
    toast.set_kind(kind);
    toast.set_revision(toast.get_revision().wrapping_add(1));
}

/// Acknowledged success: `zh` on the Chinese shell, `en` otherwise.
pub fn success(app: &MainWindow, zh: &str, en: &str) {
    notify(app, if app.get_ui_zh() { zh } else { en }, KIND_SUCCESS);
}

/// Backend errors carry their own text (`无法…` / `…失败：…`); surface it
/// verbatim, exactly as the inline error slots do.
pub fn failure(app: &MainWindow, message: impl Into<SharedString>) {
    notify(app, message, KIND_ERROR);
}

/// Turn one `status` write into a toast kind, or `None` when the text is
/// progress or startup chatter rather than a result the user is waiting for.
pub(crate) fn classify(message: &str) -> Option<i32> {
    let text = message.trim();
    if text.is_empty() || is_chatter(text) {
        return None;
    }
    Some(if is_failure(text) { KIND_ERROR } else { KIND_INFO })
}

/// Progress reports and boot banners. They are replaced by a result later, so
/// echoing them as toasts would strobe over the transcript.
// The demo banner is the window's initial `status`, so a language flip at
// startup re-evaluates it; both spellings must stay out of the toast.
const QUIET_PREFIXES: &[&str] = &["正在", "本地演示", "Local demo", "hide-probe"];

/// Words that mark a write as something the user must read (a failure, a
/// rejection or a blocked action) instead of a plain result.
const FAILURE_MARKERS: &[&str] = &[
    "失败", "错误", "无法", "不能", "未保存", "异常", "不可用", "不支持", "不允许", "未就绪",
    "尚未", "为空", "必须", "超出", "已达到", "最多可", "请先", "请填写", "请选择", "请移除",
    "请勿", "仅支持", "⚠",
];

fn is_chatter(text: &str) -> bool {
    QUIET_PREFIXES.iter().any(|prefix| text.starts_with(prefix))
}

fn is_failure(text: &str) -> bool {
    FAILURE_MARKERS.iter().any(|marker| text.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::{classify, KIND_ERROR, KIND_INFO};

    #[test]
    fn results_surface_as_information() {
        assert_eq!(classify("线程已还原"), Some(KIND_INFO));
        assert_eq!(classify("已永久删除归档线程"), Some(KIND_INFO));
        assert_eq!(classify("工具操作完成"), Some(KIND_INFO));
        assert_eq!(classify("已打开工作区文件"), Some(KIND_INFO));
        assert_eq!(classify("已复制"), Some(KIND_INFO));
        assert_eq!(classify("已归档 3 个线程"), Some(KIND_INFO));
    }

    #[test]
    fn failures_surface_as_errors_even_without_the_word_failed() {
        assert_eq!(classify("无法读取会话：disk full"), Some(KIND_ERROR));
        assert_eq!(classify("目录加载失败：denied"), Some(KIND_ERROR));
        assert_eq!(classify("已归档 2 个线程，其余失败：locked"), Some(KIND_ERROR));
        assert_eq!(classify("请先选择或新建云端线程"), Some(KIND_ERROR));
        assert_eq!(classify("Cron 表达式不能为空"), Some(KIND_ERROR));
    }

    #[test]
    fn progress_and_boot_banners_stay_quiet() {
        assert_eq!(classify(""), None);
        assert_eq!(classify("   "), None);
        assert_eq!(classify("正在打开线程，读取历史记录…"), None);
        assert_eq!(classify("正在移动 12 项…"), None);
        assert_eq!(classify("本地演示 · 未连接后端"), None);
        assert_eq!(classify("Local demo · backend not connected"), None);
        assert_eq!(classify("hide-probe"), None);
    }

    #[test]
    fn leading_space_does_not_change_the_verdict() {
        assert_eq!(classify("  线程已还原 "), Some(KIND_INFO));
        assert_eq!(classify("  无法打开文件夹：denied"), Some(KIND_ERROR));
    }
}
