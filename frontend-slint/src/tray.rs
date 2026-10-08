//! Native system tray. Keeping the generated tray object in UI-thread TLS is
//! required so its Win32 Drop implementation can issue NIM_DELETE before the
//! event loop disappears (the Win7 stale-icon workaround).

use slint::ComponentHandle;
use std::cell::RefCell;

use crate::{AppTray, MainWindow, TrayNotice};

const DEFAULT_TOOLTIP: &str = "蜂窝";
const HIDDEN_TOOLTIP: &str = "蜂窝仍在后台运行；点击恢复，右键菜单可截图或退出";

thread_local! {
    static TRAY: RefCell<Option<AppTray>> = const { RefCell::new(None) };
    static NOTICE: RefCell<Option<TrayNotice>> = const { RefCell::new(None) };
    static NOTICE_TIMER: slint::Timer = slint::Timer::default();
}

pub fn install(app: &MainWindow) {
    let tray = match AppTray::new() {
        Ok(tray) => tray,
        Err(error) => {
            app.set_status(format!("系统托盘不可用：{error:?}").into());
            return;
        }
    };
    const ICON_RGBA: &[u8] = include_bytes!("../assets/app-icon.rgba");
    tray.set_tray_icon(slint::Image::from_rgba8(
        slint::SharedPixelBuffer::clone_from_slice(ICON_RGBA, 32, 32),
    ));
    let weak = app.as_weak();
    tray.on_show_main_requested(move || {
        if let Some(app) = weak.upgrade() {
            let _ = app.show();
            app.window().request_redraw();
            hide_notice();
        }
    });
    let weak = app.as_weak();
    tray.on_screenshot_requested(move || {
        // TrackPopupMenu has just returned, but Explorer may still be painting
        // the dismissing menu. Capture on the next quarter-second tick so the
        // tray menu itself never becomes part of the user attachment.
        slint::Timer::single_shot(std::time::Duration::from_millis(240), {
            let weak = weak.clone();
            move || crate::screenshot::capture(weak, false)
        });
    });
    let weak = app.as_weak();
    tray.on_fullscreen_screenshot_requested(move || {
        // Keep the same delay as the region action so the native menu is no
        // longer part of the captured frame after TrackPopupMenu returns.
        slint::Timer::single_shot(std::time::Duration::from_millis(240), {
            let weak = weak.clone();
            move || crate::screenshot::capture_fullscreen(weak, false)
        });
    });
    // This callback is entered from the Win32 tray window's modal
    // `TrackPopupMenu` stack. On Win7, asking winit to unwind that same stack
    // can leave the hidden tray/message thread resident. Use the Windows
    // process-exit boundary for this one native-menu action; the OS then reclaims
    // the tray HWND, hotkey queue and any detached worker threads in one
    // operation. Window-close exit shares the same boundary.
    tray.on_quit_requested(crate::shutdown::request_exit);
    TRAY.with(|slot| *slot.borrow_mut() = Some(tray));
    let weak = app.as_weak();
    crate::hotkey::install(move || {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || crate::screenshot::capture(weak, false));
    });
}

#[allow(dead_code)] // Windows 退出统一走 TerminateProcess 进程边界，托盘图标由 OS 回收，无需显式注销。
pub fn uninstall() {
    hide_notice();
    TRAY.with(|slot| {
        let _ = slot.borrow_mut().take();
    });
}

/// Hide the main window while retaining the live tray icon and event loop.
/// Returning a boolean lets the window-close handler fall back to a full exit
/// when a platform does not expose a usable system tray.
pub fn hide(app: &MainWindow) -> bool {
    if !TRAY.with(|slot| slot.borrow().is_some()) {
        return false;
    }
    match app.hide() {
        Ok(()) => {
            app.set_status("已最小化到系统托盘；点击托盘图标可恢复窗口".into());
            announce(HIDDEN_TOOLTIP);
            show_notice();
            true
        }
        Err(error) => {
            app.set_status(format!("无法最小化到系统托盘：{error}").into());
            false
        }
    }
}

/// Update the tray tooltip while the main window is hidden. This remains
/// useful on shells that suppress notification balloons, and returns to the
/// normal application name after a short period.
pub fn announce(message: &str) {
    let tray = TRAY.with(|slot| slot.borrow().as_ref().map(|tray| tray.clone_strong()));
    let Some(tray) = tray else { return };
    tray.set_tray_tooltip(message.into());
    let weak = tray.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
        if let Some(tray) = weak.upgrade() {
            tray.set_tray_tooltip(DEFAULT_TOOLTIP.into());
        }
    });
}

/// Hide the transient tray notice and cancel its expiry timer.
fn hide_notice() {
    NOTICE_TIMER.with(slint::Timer::stop);
    NOTICE.with(|slot| {
        if let Some(notice) = slot.borrow().as_ref() {
            let _ = notice.hide();
        }
    });
}

/// Show a compact, actionable reminder after the main window is moved to the
/// tray. It is a normal Slint window so the behavior remains visible even when
/// the desktop shell suppresses native notification balloons.
fn show_notice() {
    let notice = NOTICE.with(|slot| {
        if slot.borrow().is_none() {
            let Ok(notice) = TrayNotice::new() else { return None };
            let weak = notice.as_weak();
            notice.on_acknowledged(move || {
                NOTICE_TIMER.with(slint::Timer::stop);
                if let Some(notice) = weak.upgrade() {
                    let _ = notice.hide();
                }
            });
            notice.on_quit(crate::shutdown::request_exit);
            *slot.borrow_mut() = Some(notice);
        }
        slot.borrow().as_ref().map(|notice| notice.clone_strong())
    });
    let Some(notice) = notice else { return };

    use slint::winit_030::WinitWindowAccessor;
    notice.window().with_winit_window(|native| {
        native.set_min_inner_size(Some(winit::dpi::LogicalSize::new(380.0, 168.0)));
    });
    notice.window().set_size(slint::LogicalSize::new(420.0, 172.0));
    if notice.show().is_err() {
        return;
    }

    // Position it just above the taskbar on the monitor hosting the notice.
    // Mapping is asynchronous, so defer until the native size is available.
    let weak = notice.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
        if let Some(notice) = weak.upgrade() {
            notice.window().with_winit_window(|native| {
                if let Some(monitor) = native.current_monitor() {
                    let monitor_size = monitor.size();
                    let monitor_origin = monitor.position();
                    let window_size = native.outer_size();
                    native.set_outer_position(winit::dpi::PhysicalPosition::new(
                        monitor_origin.x + monitor_size.width as i32 - window_size.width as i32 - 24,
                        monitor_origin.y + monitor_size.height as i32 - window_size.height as i32 - 64,
                    ));
                }
            });
        }
    });

    let weak = notice.as_weak();
    NOTICE_TIMER.with(|timer| {
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_secs(8),
            move || {
                if let Some(notice) = weak.upgrade() {
                    let _ = notice.hide();
                }
            },
        );
    });
}
