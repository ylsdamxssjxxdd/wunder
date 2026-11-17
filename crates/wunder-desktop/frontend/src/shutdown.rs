//! 退出路径：托盘菜单"退出"、驻留提示"退出程序"与窗口关闭退出共用。
//!
//! Windows（尤其 Win7）上托盘菜单回调运行在 Shell 隐藏 tray 窗口的
//! TrackPopupMenu 栈内；优雅退出后该栈与消息泵仍可能驻留，进程无法真正结束。
//! 因此先显式注销托盘，再以 TerminateProcess 结束进程，不等待 winit 栈回卷、
//! TLS 析构或游离工作线程汇合。其他平台没有原生 tray 消息窗口，走优雅退出。

pub fn request_exit() {
    #[cfg(windows)]
    {
        // 托盘菜单回调仍持有组件强引用，同步 take 不会立刻触发 Drop/NIM_DELETE。
        // 到下一轮事件循环时 TrackPopupMenu 与回调均已返回，再注销并结束进程。
        slint::Timer::single_shot(std::time::Duration::ZERO, || {
            crate::tray::uninstall();
            crate::hotkey::stop();
            unsafe {
                use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
                let process = GetCurrentProcess();
                if TerminateProcess(process, 0) == 0 {
                    // 对当前进程伪句柄几乎不可达；被加固宿主拒绝时退回 CRT 边界。
                    std::process::exit(0);
                }
            }
        });
        return;
    }

    #[cfg(not(windows))]
    graceful_exit();
}

/// 非 Windows 后端没有需要拆除的原生 tray 消息窗口，走正常退出流程：
/// 显式注销托盘图标与全局热键，再停止事件循环。
#[cfg(not(windows))]
fn graceful_exit() {
    crate::tray::uninstall();
    crate::hotkey::stop();
    let _ = slint::quit_event_loop();
}
