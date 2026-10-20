//! Native window gestures behind the custom chrome (frameless main window).
//! On Windows these call the HWND directly: winit's deferred state queue can
//! rewrite the Win7 non-client styles around a button press and lose the
//! requested transition (same rationale as the rcho workbench chrome).

#[cfg(windows)]
use windows_sys::Win32::Foundation::HWND;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AttachThreadInput, BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    IsZoomed, SetForegroundWindow, ShowWindow, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
};

/// Starts the platform caption-drag for the custom title bar. Winit turns this
/// into the native Win32 caption gesture on Windows and asks the compositor on
/// Linux, preserving snap and monitor transitions.
pub fn begin_system_move(window: &slint::Window) -> bool {
    use slint::winit_030::WinitWindowAccessor;

    window
        .with_winit_window(|native| native.drag_window().is_ok())
        .unwrap_or(false)
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
        ShowWindow(hwnd, if IsZoomed(hwnd) != 0 { SW_RESTORE } else { SW_MAXIMIZE });
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
        if foreground.is_null() || foreground == hwnd {
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

#[cfg(windows)]
fn native_hwnd(window: &slint::Window) -> Option<HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle();
    let RawWindowHandle::Win32(handle) = handle.window_handle().ok()?.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as HWND)
}
