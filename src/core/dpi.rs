#[cfg(windows)]
pub fn init_process_dpi_awareness() {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::BOOL;
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleW, GetProcAddress, LoadLibraryW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;

    type SetProcessDpiAwarenessContextFn = unsafe extern "system" fn(isize) -> BOOL;
    type SetProcessDpiAwarenessFn = unsafe extern "system" fn(i32) -> i32;

    const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: isize = -4;
    const PROCESS_PER_MONITOR_DPI_AWARE: i32 = 2;

    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| unsafe {
        // Use runtime symbol lookup so Win7 can start even when newer DPI APIs are absent.
        if let Some(set_process_dpi_awareness_context) =
            load_user32_set_process_dpi_awareness_context()
        {
            if set_process_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) != 0 {
                return;
            }
        }

        if let Some(set_process_dpi_awareness) = load_shcore_set_process_dpi_awareness() {
            if set_process_dpi_awareness(PROCESS_PER_MONITOR_DPI_AWARE) >= 0 {
                return;
            }
        }

        let _ = SetProcessDPIAware();
    });

    unsafe fn load_user32_set_process_dpi_awareness_context(
    ) -> Option<SetProcessDpiAwarenessContextFn> {
        let module = load_library(wide_null("user32.dll").as_slice());
        if module == 0 {
            return None;
        }
        let proc = GetProcAddress(module, c"SetProcessDpiAwarenessContext".as_ptr().cast());
        proc.map(|proc| std::mem::transmute::<_, SetProcessDpiAwarenessContextFn>(proc))
    }

    unsafe fn load_shcore_set_process_dpi_awareness() -> Option<SetProcessDpiAwarenessFn> {
        let module = load_library(wide_null("shcore.dll").as_slice());
        if module == 0 {
            return None;
        }
        let proc = GetProcAddress(module, c"SetProcessDpiAwareness".as_ptr().cast());
        proc.map(|proc| std::mem::transmute::<_, SetProcessDpiAwarenessFn>(proc))
    }

    unsafe fn load_library(name: &[u16]) -> isize {
        let module = GetModuleHandleW(name.as_ptr());
        if module != 0 {
            module
        } else {
            LoadLibraryW(name.as_ptr())
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn init_process_dpi_awareness() {}
