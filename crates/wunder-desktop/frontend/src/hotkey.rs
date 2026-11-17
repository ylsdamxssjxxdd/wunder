//! Win7-compatible global screenshot hotkey. A dedicated Win32 message queue
//! keeps RegisterHotKey off the Slint event loop and is explicitly stopped.

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(windows)]
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
#[cfg(windows)]
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

#[cfg(windows)]
pub fn install(handler: impl Fn() + Send + 'static) {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_CONTROL, MOD_NOREPEAT, VK_F1,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMessageW, PostThreadMessageW, MSG, WM_HOTKEY, WM_QUIT,
    };
    STOP_REQUESTED.store(false, Ordering::Release);
    std::thread::spawn(move || unsafe {
        let id = GetCurrentThreadId();
        THREAD_ID.store(id, Ordering::Release);
        if STOP_REQUESTED.load(Ordering::Acquire) {
            THREAD_ID.store(0, Ordering::Release);
            return;
        }
        if RegisterHotKey(0, 1, MOD_CONTROL | MOD_NOREPEAT, VK_F1 as u32) == 0 {
            THREAD_ID.store(0, Ordering::Release);
            return;
        }
        if STOP_REQUESTED.load(Ordering::Acquire) {
            PostThreadMessageW(id, WM_QUIT, 0, 0);
        }
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, 0, 0, 0) > 0 {
            if message.message == WM_HOTKEY {
                handler();
            }
        }
        UnregisterHotKey(0, 1);
        THREAD_ID.store(0, Ordering::Release);
    });
}

#[cfg(windows)]
pub fn stop() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
    STOP_REQUESTED.store(true, Ordering::Release);
    let id = THREAD_ID.load(Ordering::Acquire);
    if id != 0 {
        unsafe {
            PostThreadMessageW(id, WM_QUIT, 0, 0);
        }
    }
}

#[cfg(not(windows))]
pub fn install(_handler: impl Fn() + Send + 'static) {}

#[cfg(not(windows))]
pub fn stop() {}
