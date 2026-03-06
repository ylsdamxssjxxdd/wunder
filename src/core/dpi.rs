#[cfg(windows)]
pub fn init_process_dpi_awareness() {
    use std::sync::OnceLock;
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwareness, SetProcessDpiAwarenessContext,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, PROCESS_PER_MONITOR_DPI_AWARE,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;

        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            let _ = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
            let _ = SetProcessDPIAware();
        }
    });
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn init_process_dpi_awareness() {}
