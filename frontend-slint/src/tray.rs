//! Native system tray. Keeping the generated tray object in UI-thread TLS is
//! required so its Win32 Drop implementation can issue NIM_DELETE before the
//! event loop disappears (the Win7 stale-icon workaround).

use slint::ComponentHandle;
use std::cell::RefCell;

use crate::{AppTray, MainWindow};

thread_local! {
    static TRAY: RefCell<Option<AppTray>> = const { RefCell::new(None) };
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
        }
    });
    let weak = app.as_weak();
    tray.on_screenshot_requested(move || {
        // TrackPopupMenu has just returned, but Explorer may still be painting
        // the dismissing menu. Capture on the next quarter-second tick so the
        // tray menu itself never becomes part of the user attachment.
        slint::Timer::single_shot(std::time::Duration::from_millis(240), {
            let weak = weak.clone();
            move || crate::screenshot::capture(weak)
        });
    });
    let weak = app.as_weak();
    tray.on_fullscreen_screenshot_requested(move || {
        // Keep the same delay as the region action so the native menu is no
        // longer part of the captured frame after TrackPopupMenu returns.
        slint::Timer::single_shot(std::time::Duration::from_millis(240), {
            let weak = weak.clone();
            move || crate::screenshot::capture_fullscreen(weak)
        });
    });
    tray.on_quit_requested(|| {
        // The callback is invoked while the native TrackPopupMenu stack is
        // active. Defer destruction until that stack unwinds; this is the
        // Win7-safe equivalent of rcho's explicit tray teardown path.
        #[cfg(windows)]
        {
            // Win7 can keep the shell's TrackPopupMenu/message window alive
            // while the callback unwinds. The OS process boundary is the
            // reliable final cleanup for this one nested native-menu path;
            // normal close paths still explicitly drop the tray first.
            std::process::exit(0);
        }
        #[cfg(not(windows))]
        slint::Timer::single_shot(
            std::time::Duration::from_millis(20),
            crate::shutdown::request_exit,
        );
    });
    TRAY.with(|slot| *slot.borrow_mut() = Some(tray));
    let weak = app.as_weak();
    crate::hotkey::install(move || {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || crate::screenshot::capture(weak));
    });
}

pub fn uninstall() {
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
            true
        }
        Err(error) => {
            app.set_status(format!("无法最小化到系统托盘：{error}").into());
            false
        }
    }
}
