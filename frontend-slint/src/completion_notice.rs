//! Task-completion system notice, the desktop counterpart of the web
//! `notifyAgentTaskCompleted`: when a thread this process observed in flight
//! (running or pending) settles to a successful terminal state, a compact
//! frameless toast above the taskbar announces it. Clicking the toast opens
//! the main window on that session; the toast hides itself after a few
//! seconds. Errors keep the pet's warning bubble as their channel — the web
//! predicate only notifies on success.

use crate::runtime_phase::Phase;
use crate::{CompletionNotice, I18n, MainWindow};
use slint::{ComponentHandle, Model};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

/// How long one notice stays on screen before hiding itself.
const SHOW_DURATION: Duration = Duration::from_secs(4);
/// Right/bottom margin from the work-area edge, matching the tray notice.
const SCREEN_MARGIN: f64 = 24.0;
/// Card corner radius in logical pixels, mirroring `Theme.radius-md` in
/// `ui/completion-notice.slint`. The window region has to match the card or the
/// clipped silhouette and the painted border disagree at the corners.
const CARD_RADIUS: f64 = 10.0;

thread_local! {
    /// Last observed runtime kind per session, the desktop counterpart of the
    /// web `agentRuntimeStateSnapshot`. The first sight of a session only
    /// records its kind: a snapshot is history, never news, so a fresh start
    /// cannot replay old completions.
    static LAST_KIND: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
    static NOTICE: RefCell<Option<CompletionNotice>> = RefCell::new(None);
    static HIDE_TIMER: slint::Timer = slint::Timer::default();
    /// The session the visible toast opens on click.
    static OPEN_SESSION: RefCell<String> = RefCell::new(String::new());
    /// Smoke runs drive full turns and compare screenshots, so their windows
    /// must never pop a toast; `install_ready_without_pets` turns this off.
    static ENABLED: RefCell<bool> = const { RefCell::new(true) };
    /// Set while the notice component is being built. The platform hook reads
    /// it once, when the window adapter (and with it the native window
    /// attributes) is created, to mark this window as a non-activating overlay
    /// instead of an ordinary top-level.
    static NOTICE_WINDOW_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Claim the pending notice window for the platform hook. Called from the
/// backend's window-attributes hook, exactly once per created window.
pub(crate) fn take_notice_window() -> bool {
    NOTICE_WINDOW_PENDING.with(|pending| pending.replace(false))
}

/// Web `AgentRuntimeState` collapsed to what the completion predicate needs.
/// The live phase already says whether the thread is working, so this file no
/// longer keeps a phrase table of its own.
fn kind_of(phase: Phase) -> &'static str {
    match phase {
        Phase::Done => "done",
        Phase::Pending | Phase::Running | Phase::Inquiry => "hot",
        // A user stop is terminal but not a completion, so it settles silently.
        Phase::Idle | Phase::Error | Phase::Stopped => "other",
    }
}

/// Web `shouldNotifyAgentTaskCompletion`: fire exactly when the previous
/// snapshot was hot and the new kind is a successful terminal.
fn completion_fires(previous: &str, next: &str) -> bool {
    previous == "hot" && next == "done"
}

/// Observe one thread status transition.
pub fn observe_status(app: &MainWindow, session: &str, phase: Phase) {
    if !ENABLED.with(|slot| *slot.borrow()) {
        return;
    }
    let next = kind_of(phase);
    let previous = LAST_KIND.with(|slot| {
        let mut map = slot.borrow_mut();
        map.insert(session.to_string(), next).unwrap_or("other")
    });
    if !completion_fires(previous, next) {
        return;
    }
    let agent_name = agent_display_name(app, session);
    show(app, session, &agent_name);
}

pub fn reset_status(session: &str, phase: Phase) {
    LAST_KIND.with(|slot| {
        slot.borrow_mut()
            .insert(session.to_string(), kind_of(phase));
    });
}

pub(crate) fn retain_sessions(mut keep: impl FnMut(&str) -> bool) {
    LAST_KIND.with(|slot| slot.borrow_mut().retain(|session, _| keep(session)));
}

/// Resolve the agent display name the way the web `resolveAgentDisplayName`
/// does: the bound agent's name, the default-agent label for the default key,
/// and the raw id when the directory has not loaded.
fn agent_display_name(app: &MainWindow, session: &str) -> String {
    let agent_id = app
        .get_conversations()
        .iter()
        .find(|row| row.id == session)
        .map(|row| row.agent_id.to_string())
        .unwrap_or_default();
    let key = crate::navigation_ui::agent_key(Some(agent_id.as_str())).to_string();
    if key == "__default__" {
        return app.global::<I18n>().get_common_default_agent().to_string();
    }
    app.get_agents()
        .iter()
        .find(|agent| crate::navigation_ui::agent_key(Some(agent.id.as_str())) == key.as_str())
        .map(|agent| agent.name.to_string())
        .unwrap_or(agent_id)
}

/// Create once, update, position and schedule the toast. A notice arriving
/// while one is on screen replaces it and restarts the clock, mirroring the
/// web toast stack collapsing into the newest entry.
fn show(app: &MainWindow, session: &str, agent_name: &str) {
    let i18n = app.global::<I18n>();
    let title = i18n.get_cn_notice_title().to_string();
    let body = i18n
        .invoke_cn_notice_body(slint::SharedString::from(agent_name))
        .to_string();
    let weak = NOTICE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            // The adapter — and the native window — is created inside `new()`,
            // so the platform hook must know before that call that this window
            // is a toast, not the workbench.
            NOTICE_WINDOW_PENDING.with(|pending| pending.set(true));
            let created = CompletionNotice::new();
            NOTICE_WINDOW_PENDING.with(|pending| pending.set(false));
            let Ok(notice) = created else {
                return None;
            };
            let weak_app = app.as_weak();
            notice.on_open_requested(move || {
                if let Some(app) = weak_app.upgrade() {
                    let session = OPEN_SESSION.with(|cell| cell.borrow().clone());
                    open_session(&app, &session);
                }
                hide();
            });
            *slot = Some(notice);
        }
        slot.as_ref().map(|notice| notice.as_weak())
    });
    let Some(notice) = weak.and_then(|weak| weak.upgrade()) else {
        return;
    };
    notice.set_notice_title(title.into());
    notice.set_notice_body(body.into());
    OPEN_SESSION.with(|cell| *cell.borrow_mut() = session.to_string());
    let _ = notice.show();
    shape_notice(&notice);
    position_above_taskbar(&notice);
    let hide_weak = notice.as_weak();
    HIDE_TIMER.with(|timer| {
        timer.start(slint::TimerMode::SingleShot, SHOW_DURATION, move || {
            if let Some(notice) = hide_weak.upgrade() {
                let _ = notice.hide();
            }
        });
    });
}

/// Reveal the main window on the notified session — the tray reveal plus the
/// conversation selection.
fn open_session(app: &MainWindow, session: &str) {
    let _ = app.show();
    crate::native_window::restore_and_focus(app.window());
    app.window().request_redraw();
    app.set_page(crate::DesktopPage::Messages);
    if let Some(index) = app
        .get_conversations()
        .iter()
        .position(|row| row.id == session)
    {
        app.invoke_select_conversation(index as i32);
    }
}

/// Hide the visible toast, if any.
fn hide() {
    HIDE_TIMER.with(slint::Timer::stop);
    NOTICE.with(|slot| {
        if let Some(notice) = slot.borrow().as_ref() {
            let _ = notice.hide();
        }
    });
}

/// Bottom-right of the work area, the same placement idea as the tray notice:
/// above the taskbar on the primary monitor. Mapping is asynchronous, so the
/// size is read on the next tick like `tray.rs` does.
fn position_above_taskbar(notice: &CompletionNotice) {
    let weak = notice.as_weak();
    slint::Timer::single_shot(Duration::from_millis(50), move || {
        let Some(notice) = weak.upgrade() else { return };
        // The client rect only exists once the window is mapped, so the rounded
        // silhouette is (re)applied here; `show()` already tried it eagerly to
        // keep the first frames from showing square corners.
        shape_notice(&notice);
        let Some(area) = crate::pet_window::work_area() else {
            return;
        };
        // The work area is logical; the mapped window size is physical, so it
        // comes back to logical units before the corner math.
        let size = notice.window().size();
        let scale = f64::from(notice.window().scale_factor().max(1.0));
        let width = f64::from(size.width) / scale;
        let height = f64::from(size.height) / scale;
        let x = ((area.0 + area.2 - SCREEN_MARGIN - width).max(area.0 + SCREEN_MARGIN)) as f32;
        let y = ((area.1 + area.3 - SCREEN_MARGIN - height).max(area.1 + SCREEN_MARGIN)) as f32;
        notice
            .window()
            .set_position(slint::WindowPosition::Logical(slint::LogicalPosition::new(
                x, y,
            )));
    });
}

/// Match the native window silhouette to the card's rounded corners. The
/// software renderer cannot composite the transparent corner pixels, so without
/// the region they reach the desktop as black notches.
fn shape_notice(notice: &CompletionNotice) {
    let scale = f64::from(notice.window().scale_factor().max(1.0));
    crate::native_window::apply_rounded_region(notice.window(), (CARD_RADIUS * scale) as f32);
}

/// Smoke runs must never pop toasts over their screenshot comparisons.
pub fn set_enabled(enabled: bool) {
    ENABLED.with(|slot| *slot.borrow_mut() = enabled);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_phases_match_the_web_runtime_states() {
        assert_eq!(kind_of(Phase::of("running")), "hot");
        assert_eq!(kind_of(Phase::of("正在生成…")), "hot");
        assert_eq!(
            kind_of(Phase::of("请求中")),
            "hot",
            "a submission is still hot"
        );
        assert_eq!(kind_of(Phase::of("任务已排队…")), "hot");
        assert_eq!(kind_of(Phase::of("等待用户输入")), "hot");
        assert_eq!(kind_of(Phase::of("输出已结束")), "done");
        assert_eq!(kind_of(Phase::of("任务完成")), "done");
        assert_eq!(kind_of(Phase::of("completed")), "done");
        assert_eq!(
            kind_of(Phase::of("已停止")),
            "other",
            "cancelled is not completion"
        );
        assert_eq!(
            kind_of(Phase::of("执行失败")),
            "other",
            "errors keep the pet bubble"
        );
        assert_eq!(kind_of(Phase::of("")), "other");
    }

    #[test]
    fn only_a_hot_to_done_transition_fires() {
        assert!(!completion_fires("other", "done"), "first sight is history");
        assert!(!completion_fires("hot", "hot"));
        assert!(!completion_fires("hot", "other"));
        assert!(completion_fires("hot", "done"));
        assert!(!completion_fires("done", "done"), "no repeat notices");
        assert!(!completion_fires("done", "hot"));
    }
}
