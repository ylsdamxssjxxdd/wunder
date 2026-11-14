//! Task-completion system notice, the desktop counterpart of the web
//! `notifyAgentTaskCompleted`: when a thread this process observed in flight
//! (running or pending) settles to a successful terminal state, a compact
//! frameless toast above the taskbar announces it. On Wayland sessions the
//! floating window cannot be positioned, so there a notification-daemon
//! banner takes its place. Clicking either opens the main window on that
//! session; the notice hides itself after a few seconds. Errors keep the
//! pet's warning bubble as their channel — the web predicate only notifies
//! on success.

use crate::runtime_phase::Phase;
use crate::{CompletionNotice, I18n, MainWindow};
use slint::{ComponentHandle, Model};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long one notice stays on screen before hiding itself.
const SHOW_DURATION: Duration = Duration::from_secs(4);
/// How long a just-watched outcome still counts as news for the companion and
/// the status fence. The sidebar row itself keeps the outcome until the next
/// turn; freshness is a notification concern, never a row-state concern.
const SETTLE_NOTICE_HOLD: Duration = Duration::from_secs(6);
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
    /// Sessions whose outcome this window just watched, with the instant the
    /// news stops being fresh. Sized by the number of in-flight threads.
    static RECENT_SETTLES: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
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

/// Mark a settlement this window actually watched. Only the notification side
/// uses it: the durable row keeps `done` until the next turn starts.
pub fn mark_recent_settle(session: &str) {
    if session.is_empty() {
        return;
    }
    RECENT_SETTLES.with(|slot| {
        slot.borrow_mut()
            .insert(session.to_string(), Instant::now() + SETTLE_NOTICE_HOLD);
    });
}

/// Whether this session's outcome is still news to the companion sprite.
pub fn recent_settle_active(session: &str) -> bool {
    RECENT_SETTLES.with(|slot| {
        slot.borrow()
            .get(session)
            .is_some_and(|until| *until > Instant::now())
    })
}

/// Retire expired news. Returns the sessions whose hold ended (their status
/// fence can now be released) and whether any news is still pending, so the
/// caller only keeps a timer alive while there is something to expire.
pub fn expire_recent_settles() -> (Vec<String>, bool) {
    RECENT_SETTLES.with(|slot| {
        let mut recent = slot.borrow_mut();
        let now = Instant::now();
        let expired: Vec<String> = recent
            .iter()
            .filter(|(_, until)| **until <= now)
            .map(|(session, _)| session.clone())
            .collect();
        for session in &expired {
            recent.remove(session);
        }
        (expired, !recent.is_empty())
    })
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
    // On Wayland the compositor owns top-level placement: a floating toast
    // window cannot be positioned at all and pops up wherever the shell
    // pleases. There the notice goes through the notification daemon, whose
    // banner position is the shell's own and always the same.
    #[cfg(target_os = "linux")]
    if crate::pet_window::wayland_session() && show_system_notice(&title, &body, app, session) {
        return;
    }
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
        #[cfg(windows)]
        {
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
            notice.window().set_position(slint::WindowPosition::Logical(
                slint::LogicalPosition::new(x, y),
            ));
        }
        #[cfg(not(windows))]
        position_via_winit(&notice);
    });
}

/// Non-Windows counterpart of the work-area math: bottom-right of the monitor
/// the toast landed on, through winit's physical coordinates. Wayland never
/// reaches here — the compositor discards top-level positions, so that session
/// takes the D-Bus notification path in `show_system_notice` instead.
#[cfg(not(windows))]
fn position_via_winit(notice: &CompletionNotice) {
    use slint::winit_030::WinitWindowAccessor;
    notice.window().with_winit_window(|native| {
        let Some(monitor) = native.current_monitor() else {
            return;
        };
        let scale = f64::from(native.scale_factor());
        let origin = monitor.position();
        let monitor_size = monitor.size();
        let window_size = native.outer_size();
        // Right margin keeps the SCREEN_MARGIN look; the deeper bottom margin
        // clears the tall panels many X11 desktops dock at the screen bottom,
        // the same clearance the tray notice uses.
        let margin_x = (SCREEN_MARGIN * scale) as i32;
        let margin_y = ((SCREEN_MARGIN + 40.0) * scale) as i32;
        native.set_outer_position(winit::dpi::PhysicalPosition::new(
            origin.x + monitor_size.width as i32 - window_size.width as i32 - margin_x,
            origin.y + monitor_size.height as i32 - window_size.height as i32 - margin_y,
        ));
    });
}

/// Emit the notice through the desktop notification daemon. The banner's
/// position is the shell's own and deterministic; clicking it (the default
/// action or the 打开 button) opens the notified session. Returns `false` when
/// no daemon could be reached, letting the caller fall back to the toast
/// window. Signal listening lives in one app-wide thread (see
/// [`daemon::PENDING`]) — per-notice wait threads would pile up on shells that
/// keep notifications resident after the banner expires and never send
/// `NotificationClosed`.
#[cfg(target_os = "linux")]
fn show_system_notice(title: &str, body: &str, app: &MainWindow, session: &str) -> bool {
    daemon::show(title, body, app.as_weak(), session)
}

/// Session-bus plumbing for the notification-daemon notice. One shared
/// connection, one listener thread, and the single pending notice whose
/// actions are honoured — a newer notice simply replaces it, mirroring the
/// way the window toast collapses into the newest entry.
#[cfg(target_os = "linux")]
mod daemon {
    use super::{open_session, MainWindow, SHOW_DURATION};
    use slint::Weak;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    struct Pending {
        id: u32,
        session: String,
        weak: Weak<MainWindow>,
    }

    /// The daemon connection, created once. `None` marks a session bus that
    /// was unreachable, so later notices skip straight to the window fallback.
    static CONNECTION: OnceLock<Option<zbus::blocking::Connection>> = OnceLock::new();
    static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

    fn connection() -> Option<&'static zbus::blocking::Connection> {
        let started = CONNECTION.get_or_init(|| {
            let Ok(conn) = zbus::blocking::Connection::session() else {
                return None;
            };
            start_listener(conn.clone());
            Some(conn)
        });
        started.as_ref()
    }

    /// Route daemon signals to the pending notice. The iterator blocks for the
    /// lifetime of the process, which is exactly the bound a singleton needs.
    fn start_listener(conn: zbus::blocking::Connection) {
        std::thread::spawn(move || {
            let Ok(rule) = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.Notifications")
                .and_then(|rule| rule.interface("org.freedesktop.Notifications"))
                .and_then(|rule| Ok(rule.build()))
            else {
                return;
            };
            let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, None)
            else {
                return;
            };
            for message in messages {
                let Ok(message) = message else { continue };
                let member = message
                    .header()
                    .member()
                    .map(|name| name.as_str().to_string())
                    .unwrap_or_default();
                match member.as_str() {
                    "ActionInvoked" => {
                        let Ok((id, action)) = message.body().deserialize::<(u32, String)>() else {
                            continue;
                        };
                        if action != "default" && action != "打开" {
                            continue;
                        }
                        let invoked = PENDING.lock().ok().and_then(|guard| {
                            let pending = guard.as_ref()?;
                            (pending.id == id)
                                .then(|| (pending.session.clone(), pending.weak.clone()))
                        });
                        if let Some((session, weak)) = invoked {
                            PENDING.lock().ok().and_then(|mut guard| {
                                match guard.as_ref() {
                                    Some(pending) if pending.id == id => *guard = None,
                                    _ => {}
                                }
                                Some(())
                            });
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = weak.upgrade() {
                                    open_session(&app, &session);
                                }
                            });
                        }
                    }
                    "NotificationClosed" => {
                        let Ok((id, _reason)) = message.body().deserialize::<(u32, u32)>() else {
                            continue;
                        };
                        PENDING.lock().ok().and_then(|mut guard| {
                            match guard.as_ref() {
                                Some(pending) if pending.id == id => *guard = None,
                                _ => {}
                            }
                            Some(())
                        });
                    }
                    _ => {}
                }
            }
        });
    }

    /// Post the notice and mark it as the one whose clicks are honoured.
    pub(super) fn show(title: &str, body: &str, weak: Weak<MainWindow>, session: &str) -> bool {
        let Some(conn) = connection() else {
            return false;
        };
        let Ok(proxy) = zbus::blocking::Proxy::new(
            conn,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        ) else {
            return false;
        };
        // "default" is what the shell fires when the banner body is clicked;
        // the labelled entry covers shells that render the actions as buttons.
        let actions: &[&str] = &["default", "打开"];
        let hints = HashMap::<&str, zbus::zvariant::Value>::new();
        let id: u32 = match proxy.call(
            "Notify",
            &(
                "wunder 桌面版",
                0u32,
                "",
                title,
                body,
                actions,
                hints,
                SHOW_DURATION.as_millis() as i32,
            ),
        ) {
            Ok(id) => id,
            Err(_) => return false,
        };
        if let Ok(mut guard) = PENDING.lock() {
            *guard = Some(Pending {
                id,
                session: session.to_string(),
                weak,
            });
        }
        true
    }
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

    /// Freshness is a notification concern only. Expiring the news hands back
    /// an id so the caller can release the status fence; it can never rewrite
    /// the durable row.
    #[test]
    fn recent_settle_expires_without_touching_the_durable_row() {
        let session = "fixture-settled-thread";
        assert!(!recent_settle_active(session));
        mark_recent_settle(session);
        assert!(recent_settle_active(session));
        let (expired, pending) = expire_recent_settles();
        assert!(expired.is_empty(), "a fresh outcome is not expired yet");
        assert!(
            pending,
            "the notice timer stays alive while news is pending"
        );

        RECENT_SETTLES.with(|slot| {
            slot.borrow_mut()
                .insert(session.to_string(), Instant::now() - Duration::from_secs(1));
        });
        let (expired, pending) = expire_recent_settles();
        assert_eq!(expired, vec![session.to_string()]);
        assert!(!pending, "an idle app owns no timer");
        assert!(!recent_settle_active(session));
    }

    #[test]
    fn an_unknown_session_has_no_recent_outcome() {
        assert!(!recent_settle_active("fixture-never-ran"));
        mark_recent_settle("");
        assert!(!recent_settle_active(""));
    }
}
