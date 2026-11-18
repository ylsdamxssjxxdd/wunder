//! 工作台截图快捷键：F1 先隐藏主窗口再截图，F2 保留窗口直接截图。
//!
//! 拦截发生在窗口层——Windows 子类化主 HWND 的消息过程，X11 监听工作台窗口的按键
//! 事件——因此与 Slint 当前焦点落在哪个控件无关（输入区、时间线、侧栏都能触发），
//! 同时只在工作台窗口拥有键盘焦点时生效：其它程序里的 F1/F2 保持原有含义（F1 帮助、
//! F2 重命名），不会被工作台抢走。
//!
//! 隐藏窗口的截图仍走截图模块那条链路：主窗口隐藏后，整屏读取与 PNG 编码都在工作
//! 线程完成，结果回到 UI 线程再恢复窗口，避免把正在消失的窗口拍进画面。

use crate::MainWindow;
use slint::ComponentHandle;
use std::sync::Arc;

/// 快捷键要求的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 先隐藏工作台，截图里只剩它背后的内容。
    HideAndCapture,
    /// 保持工作台可见再截图。
    Capture,
}

/// 窗口句柄要等 winit 真正把窗口建出来才有：首帧之前 `with_winit_window`
/// 可能还没有适配器，因此安装动作在事件循环里有限次重试。
#[cfg(any(windows, target_os = "linux"))]
const HANDLE_RETRY: std::time::Duration = std::time::Duration::from_millis(50);
#[cfg(any(windows, target_os = "linux"))]
const HANDLE_ATTEMPTS: u32 = 40;

/// 安装工作台快捷键。回调可能在平台线程里被调用，调用方负责切回事件循环。
pub fn install(app: &MainWindow, handler: impl Fn(Action) + Send + Sync + 'static) {
    #[cfg(any(windows, target_os = "linux"))]
    {
        attach(app, Arc::new(handler), 0);
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (app, handler);
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn attach(app: &MainWindow, handler: Arc<dyn Fn(Action) + Send + Sync>, attempt: u32) {
    if let Some(target) = platform::target(app) {
        platform::install(target, handler);
        return;
    }
    if attempt >= HANDLE_ATTEMPTS {
        eprintln!("截图快捷键不可用：没有取得工作台窗口句柄");
        return;
    }
    let weak = app.as_weak();
    slint::Timer::single_shot(HANDLE_RETRY, move || {
        if let Some(app) = weak.upgrade() {
            attach(&app, handler, attempt + 1);
        }
    });
}

/// 退出前注销快捷键。
pub fn stop() {
    #[cfg(any(windows, target_os = "linux"))]
    platform::stop();
}

#[cfg(windows)]
mod platform {
    use super::Action;
    use slint::ComponentHandle;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::{Arc, OnceLock};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallWindowProcW, GWLP_WNDPROC, WM_KEYDOWN, WM_SYSKEYDOWN, WNDPROC,
    };
    // windows-sys 只给 64 位架构提供 `SetWindowLongPtrW`，32 位（Win7 x86）必须
    // 用 `SetWindowLongW`；两者返回的都是旧过程地址。
    #[cfg(target_pointer_width = "32")]
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongW;
    #[cfg(target_pointer_width = "64")]
    use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;

    const VK_F1: usize = 0x70;
    const VK_F2: usize = 0x71;

    type Handler = Arc<dyn Fn(Action) + Send + Sync>;

    static HANDLER: OnceLock<Handler> = OnceLock::new();
    static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

    /// 主窗口的 HWND。Win32 是单 HWND + 自绘，所以这一层拦截覆盖全部控件。
    pub fn target(app: &super::MainWindow) -> Option<isize> {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        app.window()
            .with_winit_window(|native| match native.window_handle().ok()?.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as isize),
                _ => None,
            })
            .flatten()
    }

    pub fn install(hwnd: isize, handler: Handler) {
        let _ = HANDLER.set(handler);
        // 取旧过程地址后再链式调用，winit 自己的消息处理因此不受影响。
        #[cfg(target_pointer_width = "64")]
        let previous = unsafe {
            SetWindowLongPtrW(hwnd as HWND, GWLP_WNDPROC, wndproc as *const () as isize) as isize
        };
        #[cfg(target_pointer_width = "32")]
        let previous = unsafe {
            SetWindowLongW(hwnd as HWND, GWLP_WNDPROC, wndproc as *const () as i32) as isize
        };
        if previous == 0 {
            eprintln!("截图快捷键不可用：无法挂载窗口消息过程");
            return;
        }
        PREVIOUS.store(previous, Ordering::Release);
    }

    pub fn stop() {
        // 进程退出时由系统回收 HWND 与消息过程，无需手工还原。
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            // 长按自动重复（lparam 第 30 位）只认第一次，避免连拍。
            let repeated = (lparam as usize) & (1 << 30) != 0;
            let action = match wparam {
                VK_F1 => Some(Action::HideAndCapture),
                VK_F2 => Some(Action::Capture),
                _ => None,
            };
            if let Some(action) = action {
                if !repeated {
                    if let Some(handler) = HANDLER.get() {
                        handler(action);
                    }
                }
                return 0;
            }
        }
        let previous = PREVIOUS.load(Ordering::Acquire);
        if previous == 0 {
            return 0;
        }
        let previous: WNDPROC = std::mem::transmute(previous);
        CallWindowProcW(previous, hwnd, message, wparam, lparam)
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::Action;
    use slint::ComponentHandle;
    use std::sync::{Arc, OnceLock};
    use x11_dl::{keysym, xlib};

    type Handler = Arc<dyn Fn(Action) + Send + Sync>;

    static HANDLER: OnceLock<Handler> = OnceLock::new();

    /// 工作台的 X11 窗口。Wayland 会话拿不到 XID，快捷键在那里不可用（截图本身
    /// 也只支持 X11）。Winit 的 X11 后端基于 XCB，句柄可能是 Xcb 也可能是 Xlib，
    /// 两者带的都是同一个 server 端窗口 ID。
    pub fn target(app: &super::MainWindow) -> Option<u64> {
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        app.window()
            .with_winit_window(|native| match native.window_handle().ok()?.as_raw() {
                RawWindowHandle::Xlib(handle) => Some(handle.window as u64),
                RawWindowHandle::Xcb(handle) => Some(u64::from(handle.window.get())),
                _ => None,
            })
            .flatten()
    }

    pub fn install(window: u64, handler: Handler) {
        let _ = HANDLER.set(handler);
        std::thread::spawn(move || {
            let xlib = match xlib::Xlib::open() {
                Ok(xlib) => xlib,
                Err(_) => return,
            };
            unsafe {
                // 独立的 Display 连接：Xlib 只允许同一连接在单线程里使用，抓取线程
                // 因此不会和截图线程或 winit 的连接互相干扰。
                let display = (xlib.XOpenDisplay)(std::ptr::null());
                if display.is_null() {
                    return;
                }
                // 只订阅本窗口的按键事件：X 的事件掩码按客户端独立，所以 Slint
                // 仍然照常收到同一批事件，快捷键是旁路而不是抢占。
                (xlib.XSelectInput)(
                    display,
                    window as xlib::Window,
                    xlib::KeyPressMask | xlib::KeyReleaseMask,
                );
                (xlib.XSync)(display, 0);
                let keys = [
                    (xlib.XKeysymToKeycode)(display, u64::from(keysym::XK_F1)) as u32,
                    (xlib.XKeysymToKeycode)(display, u64::from(keysym::XK_F2)) as u32,
                ];
                let mut held = [false; 2];
                loop {
                    let mut event: xlib::XEvent = std::mem::zeroed();
                    // 阻塞等待：空闲时不占用 CPU，进程退出时连接随之关闭。
                    (xlib.XNextEvent)(display, &mut event);
                    let key = event.key;
                    let slot = match keys.iter().position(|&code| code == key.keycode) {
                        Some(slot) => slot,
                        None => continue,
                    };
                    match key.type_ {
                        xlib::KeyPress => {
                            if held[slot] {
                                continue; // 长按自动重复只认第一次
                            }
                            held[slot] = true;
                            if let Some(handler) = HANDLER.get() {
                                handler(if slot == 0 {
                                    Action::HideAndCapture
                                } else {
                                    Action::Capture
                                });
                            }
                        }
                        xlib::KeyRelease => held[slot] = false,
                        _ => {}
                    }
                }
            }
        });
    }

    pub fn stop() {
        // 事件循环退出时进程随即结束，阻塞在 XNextEvent 的线程随之终止。
    }
}
