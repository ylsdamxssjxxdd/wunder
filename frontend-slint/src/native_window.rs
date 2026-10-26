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
    IsZoomed, SetForegroundWindow, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
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
    let Some(hwnd) = native_hwnd(window) else { return };
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

/// Tray restore: map the window again, undo an iconic state and pull it into
/// the foreground. `slint::Window::show()` alone re-maps the HWND but leaves
/// it behind the current foreground window (and can be dropped entirely by
/// the winit queue on Win7), which read as "the tray icon does nothing".
#[cfg(windows)]
pub fn restore_and_focus(window: &slint::Window) -> bool {
    native_hwnd(window).is_some_and(|hwnd| unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
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
    let Some(hwnd) = native_hwnd(window) else { return };
    let diameter = (radius.max(0.0) * 2.0).round() as i32;
    if diameter <= 0 {
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

#[cfg(not(windows))]
pub fn apply_rounded_region(_window: &slint::Window, _radius: f32) {}

#[cfg(windows)]
fn native_hwnd(window: &slint::Window) -> Option<HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle();
    let RawWindowHandle::Win32(handle) = handle.window_handle().ok()?.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as HWND)
}
