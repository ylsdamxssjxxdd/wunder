//! Native window gestures behind the custom chrome (frameless main window).
//! On Windows these call the HWND directly: winit's deferred state queue can
//! rewrite the Win7 non-client styles around a button press and lose the
//! requested transition (same rationale as the rcho workbench chrome).

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, RECT};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{
    CreateRoundRectRgn, DeleteObject, GetMonitorInfoW, MonitorFromWindow, SetWindowRgn,
    MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, IsZoomed, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_NOTOPMOST,
    HWND_TOPMOST, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE,
};

/// Starts the platform caption-drag for the custom title bar. Winit turns this
/// into the native Win32 caption gesture on Windows and asks the compositor on
/// Linux, preserving snap and monitor transitions.
pub fn begin_system_move(window: &slint::Window) -> bool {
    // The caption drag only starts on the active window. When another window
    // (settings/help/...) stole focus, a bare press just activates the window
    // and swallows the gesture, which read as "the title bar stopped dragging
    // until the other window is closed". Pull the window forward first so the
    // very first drag behaves like any other app.
    #[cfg(windows)]
    activate_for_drag(window);
    use slint::winit_030::WinitWindowAccessor;

    window
        .with_winit_window(|native| native.drag_window().is_ok())
        .unwrap_or(false)
}

/// Raise and activate the window before the caption drag. A plain
/// `SetForegroundWindow` is rejected under the foreground lock, so when it
/// fails the foreground thread's input state is borrowed and retried (the same
/// dance the tray restore uses).
#[cfg(windows)]
fn activate_for_drag(window: &slint::Window) {
    let Some(hwnd) = native_hwnd(window) else {
        return;
    };
    unsafe {
        if GetForegroundWindow() == hwnd {
            return;
        }
        BringWindowToTop(hwnd);
        if SetForegroundWindow(hwnd) != 0 {
            return;
        }
        let foreground = GetForegroundWindow();
        if foreground == 0 {
            return;
        }
        let current_thread = GetCurrentThreadId();
        let foreground_thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        if foreground_thread == 0 {
            return;
        }
        AttachThreadInput(current_thread, foreground_thread, 1);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        AttachThreadInput(current_thread, foreground_thread, 0);
    }
}

#[cfg(windows)]
pub fn minimize(window: &slint::Window) -> bool {
    native_hwnd(window).is_some_and(|hwnd| unsafe {
        ShowWindow(hwnd, SW_MINIMIZE);
        true
    })
}

#[cfg(not(windows))]
pub fn minimize(window: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    window
        .with_winit_window(|native| {
            native.set_minimized(true);
            true
        })
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn toggle_maximize(window: &slint::Window) -> bool {
    native_hwnd(window).is_some_and(|hwnd| unsafe {
        ShowWindow(
            hwnd,
            if IsZoomed(hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            },
        );
        true
    })
}

#[cfg(not(windows))]
pub fn toggle_maximize(window: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    window
        .with_winit_window(|native| {
            native.set_maximized(!native.is_maximized());
            true
        })
        .unwrap_or(false)
}

/// Pin/unpin the window above every other top-level window.
///
/// Windows goes through the HWND: the pin must take effect on the same frame
/// as the click, and winit defers attribute changes through its state queue.
/// The rest of the platforms ask the compositor through winit; a window manager
/// that does not implement the "above" state (Wayland) simply keeps the normal
/// stacking, which is why the caller only marks the pin active on success.
#[cfg(windows)]
pub fn set_always_on_top(window: &slint::Window, pinned: bool) -> bool {
    native_hwnd(window).is_some_and(|hwnd| unsafe {
        SetWindowPos(
            hwnd,
            if pinned { HWND_TOPMOST } else { HWND_NOTOPMOST },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        ) != 0
    })
}

#[cfg(not(windows))]
pub fn set_always_on_top(window: &slint::Window, pinned: bool) -> bool {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    window
        .with_winit_window(|native| {
            // winit's Wayland backend has no "above" state, so the request would
            // be silently dropped; report failure instead of pinning the icon.
            if matches!(
                native.window_handle().ok().map(|handle| handle.as_raw()),
                Some(RawWindowHandle::Wayland(_))
            ) {
                return false;
            }
            native.set_window_level(if pinned {
                winit::window::WindowLevel::AlwaysOnTop
            } else {
                winit::window::WindowLevel::Normal
            });
            true
        })
        .unwrap_or(false)
}

/// Tray restore: map the window again, undo an iconic state and pull it into
/// the foreground. `slint::Window::show()` alone re-maps the HWND but leaves
/// it behind the current foreground window (and can be dropped entirely by
/// the winit queue on Win7), which read as "the tray icon does nothing".
///
/// The unmapped check is the escape hatch for any path that hides the HWND
/// behind Winit's back (hidden-window capture, external tools): Slint and
/// Winit still record the window as shown, so `show()` diffs to nothing and
/// the only way back is a real mapping here.
#[cfg(windows)]
pub fn restore_and_focus(window: &slint::Window) -> bool {
    native_hwnd(window).is_some_and(|hwnd| unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else if IsWindowVisible(hwnd) == 0 {
            ShowWindow(hwnd, SW_SHOW);
        }
        let raised = BringWindowToTop(hwnd) != 0;
        if SetForegroundWindow(hwnd) != 0 {
            return true;
        }
        // Foreground lock: the tray click arrives as shell input, so a bare
        // SetForegroundWindow is rejected. Borrow the foreground thread's
        // input state and retry; without this the restored window stays
        // behind whatever the user was looking at.
        let foreground = GetForegroundWindow();
        if foreground == 0 || foreground == hwnd {
            return raised;
        }
        let current_thread = GetCurrentThreadId();
        let foreground_thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        if foreground_thread == 0 {
            return raised;
        }
        AttachThreadInput(current_thread, foreground_thread, 1);
        BringWindowToTop(hwnd);
        let focused = SetForegroundWindow(hwnd) != 0;
        AttachThreadInput(current_thread, foreground_thread, 0);
        raised || focused
    })
}

#[cfg(not(windows))]
pub fn restore_and_focus(window: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;
    window
        .with_winit_window(|native| {
            native.set_minimized(false);
            native.focus_window();
            true
        })
        .unwrap_or(false)
}

/// Whether the HWND is currently on the desktop. Win32 is authoritative here:
/// Slint's `Window::is_visible()` only reports whether a component was shown,
/// which stays true for a window that was unmapped behind its back.
#[cfg(windows)]
pub fn is_mapped(window: &slint::Window) -> bool {
    native_hwnd(window).is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd) != 0 })
}

#[cfg(not(windows))]
pub fn is_mapped(_window: &slint::Window) -> bool {
    true
}

/// Usable desktop rectangle of the window's monitor in physical pixels,
/// excluding the taskbar and other reserved areas. Other platforms fall back
/// to the full monitor bounds at the call site.
#[cfg(windows)]
pub fn work_area(window: &slint::Window) -> Option<(i32, i32, i32, i32)> {
    native_hwnd(window).and_then(|hwnd| unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor == 0 {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        Some((
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right - info.rcWork.left,
            info.rcWork.bottom - info.rcWork.top,
        ))
    })
}

#[cfg(not(windows))]
pub fn work_area(_window: &slint::Window) -> Option<(i32, i32, i32, i32)> {
    None
}

/// Clip a frameless window to a rounded rectangle so its corners are truly
/// absent instead of painted.
///
/// The software renderer presents through GDI, which carries no per-pixel
/// alpha: an antialiased corner declared `transparent` in `.slint` is handed to
/// the desktop as opaque black, so a rounded card on a translucent window shows
/// four dark notches (most visible at the top-left). Clipping the HWND to the
/// same rounded silhouette removes those pixels from the window entirely — the
/// corner shows the desktop, and the region also drops the dead pixels from hit
/// testing. GDI regions are hard-edged (no antialiasing), so `radius` is the
/// physical corner radius and the caller keeps it small.
#[cfg(windows)]
pub fn apply_rounded_region(window: &slint::Window, radius: f32) {
    let Some(hwnd) = native_hwnd(window) else {
        return;
    };
    let diameter = (radius.max(0.0) * 2.0).round() as i32;
    if diameter <= 0 {
        unsafe { SetWindowRgn(hwnd, 0, 1) };
        return;
    }
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return;
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return;
        }
        // A GDI region treats the right/bottom edge as exclusive, so the shape
        // is one pixel larger than the client rect to avoid shaving the border.
        let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
        if region == 0 {
            return;
        }
        if SetWindowRgn(hwnd, region, 1) == 0 {
            // Only a successful SetWindowRgn hands the region to the system.
            DeleteObject(region);
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn apply_rounded_region(_window: &slint::Window, _radius: f32) {}

/// Match the window manager's outline to the anti-aliased Slint surface. The
/// native region removes the rectangular manager frame; the transparent frame
/// buffer supplies the smooth pixels inside it.
#[cfg(target_os = "linux")]
pub fn apply_rounded_region(window: &slint::Window, radius: f32) {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    use x11_dl::{xfixes, xlib};

    let xwindow = window
        .with_winit_window(|native| {
            let handle = native.window_handle().ok()?;
            let RawWindowHandle::Xlib(handle) = handle.as_raw() else {
                return None;
            };
            Some((handle.window, native.inner_size()))
        })
        .flatten();
    let Some((xwindow, size)) = xwindow else {
        return;
    };
    if size.width == 0 || size.height == 0 {
        return;
    }
    let mut rectangles = rounded_rectangle_strips(size.width, size.height, radius);
    let Ok(lib) = xlib::Xlib::open() else {
        return;
    };
    let Ok(fixes) = xfixes::Xlib::open() else {
        return;
    };
    unsafe {
        let display = (lib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return;
        }
        let region = (fixes.XFixesCreateRegion)(
            display,
            rectangles.as_mut_ptr(),
            rectangles.len() as std::os::raw::c_int,
        );
        if region != 0 {
            (fixes.XFixesSetWindowShapeRegion)(display, xwindow, 0, 0, 0, region);
            (fixes.XFixesDestroyRegion)(display, region);
            (lib.XFlush)(display);
        }
        (lib.XCloseDisplay)(display);
    }
}

#[cfg(target_os = "linux")]
fn rounded_rectangle_strips(width: u32, height: u32, radius: f32) -> Vec<x11_dl::xlib::XRectangle> {
    use x11_dl::xlib::XRectangle;

    let full = || {
        vec![XRectangle {
            x: 0,
            y: 0,
            width: width.min(u16::MAX as u32) as u16,
            height: height.min(u16::MAX as u32) as u16,
        }]
    };
    let radius = radius
        .max(0.0)
        .min(width as f32 / 2.0)
        .min(height as f32 / 2.0)
        .round() as i32;
    if radius <= 0 {
        return full();
    }
    let width = width as i32;
    let height = height as i32;
    let strip = |inset: i32, y: i32, strip_height: i32| XRectangle {
        x: inset.clamp(0, i16::MAX as i32) as i16,
        y: y.clamp(0, i16::MAX as i32) as i16,
        width: (width - inset * 2).clamp(0, u16::MAX as i32) as u16,
        height: strip_height.clamp(0, u16::MAX as i32) as u16,
    };
    let corner_inset = |distance: f32| {
        radius as f32
            - (radius as f32 * radius as f32 - distance * distance)
                .max(0.0)
                .sqrt()
    };
    let mut rectangles = Vec::with_capacity((radius * 2 + 1) as usize);
    for y in 0..radius {
        let distance = radius as f32 - y as f32 - 0.5;
        let inset = corner_inset(distance).round() as i32;
        rectangles.push(strip(inset, y, 1));
    }
    rectangles.push(strip(0, radius, height - radius * 2));
    for y in height - radius..height {
        let distance = y as f32 + 0.5 - (height - radius) as f32;
        let inset = corner_inset(distance).round() as i32;
        rectangles.push(strip(inset, y, 1));
    }
    rectangles
}

#[cfg(all(test, target_os = "linux"))]
mod rounded_region_tests {
    #[test]
    fn rounded_strips_match_all_four_corners() {
        let strips = super::rounded_rectangle_strips(100, 80, 12.0);
        assert!(strips.len() > 3);
        assert!(strips.first().unwrap().x > 0);
        assert_eq!(strips.last().unwrap().x, strips.first().unwrap().x);
        assert!(strips.iter().any(|strip| {
            strip.x == 0 && strip.y == 12 && strip.width == 100 && strip.height == 56
        }));
    }

    #[test]
    fn zero_radius_restores_the_full_rectangle() {
        let strips = super::rounded_rectangle_strips(100, 80, 0.0);
        assert_eq!(strips.len(), 1);
        assert_eq!((strips[0].width, strips[0].height), (100, 80));
    }
}

/// Keep the workbench silhouette in sync with its physical size, zoom state and
/// the radius the UI paints. `radius_lg` is `Theme.radius-lg` in logical pixels
/// — the same token `window_chrome.slint` draws the frame with — so the native
/// region and the painted edge cannot drift apart if the token changes.
pub fn sync_main_window_shape(window: &slint::Window, radius_lg: f32) {
    use slint::winit_030::WinitWindowAccessor;

    let Some((scale, maximized)) =
        window.with_winit_window(|native| (native.scale_factor() as f32, native.is_maximized()))
    else {
        return;
    };
    apply_rounded_region(window, if maximized { 0.0 } else { radius_lg.max(0.0) * scale });
}

#[cfg(windows)]
fn native_hwnd(window: &slint::Window) -> Option<HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle();
    let RawWindowHandle::Win32(handle) = handle.window_handle().ok()?.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as HWND)
}

/// X11: drop `_NET_WM_STATE_DEMANDS_ATTENTION` from an auxiliary top-level.
/// Mutter raises that state when a no-activate Utility window is mapped while
/// another app holds focus, and never clears it because the overlay never takes
/// focus — the dock/taskbar icon then bounces forever. winit's
/// `request_user_attention(None)` only touches the WM_HINTS urgency bit, not the
/// EWMH state, so send the same `_NET_WM_STATE` remove message a WM client uses.
#[cfg(target_os = "linux")]
pub fn clear_demands_attention(window: &slint::Window) {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::winit_030::WinitWindowAccessor;
    use x11_dl::xlib;

    let xwindow = window
        .with_winit_window(|native| {
            let handle = native.window_handle().ok()?;
            let RawWindowHandle::Xlib(handle) = handle.as_raw() else {
                return None;
            };
            Some(handle.window)
        })
        .flatten();
    let Some(xwindow) = xwindow else { return };
    let Ok(lib) = xlib::Xlib::open() else { return };
    unsafe {
        let display = (lib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return;
        }
        let root = (lib.XRootWindow)(display, (lib.XDefaultScreen)(display));
        let state = (lib.XInternAtom)(
            display,
            b"_NET_WM_STATE\0".as_ptr() as *const std::ffi::c_char,
            0,
        );
        let demands = (lib.XInternAtom)(
            display,
            b"_NET_WM_STATE_DEMANDS_ATTENTION\0".as_ptr() as *const std::ffi::c_char,
            0,
        );
        let mut event: xlib::XEvent = std::mem::zeroed();
        event.client_message.type_ = xlib::ClientMessage;
        event.client_message.window = xwindow;
        event.client_message.message_type = state;
        event.client_message.format = 32;
        event.client_message.data.set_long(0, 0); // _NET_WM_STATE_REMOVE
        event
            .client_message
            .data
            .set_long(1, demands as std::ffi::c_long);
        (lib.XSendEvent)(
            display,
            root,
            0,
            xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
            &mut event,
        );
        (lib.XFlush)(display);
        (lib.XCloseDisplay)(display);
    }
}
