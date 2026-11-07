//! 退出路径：托盘菜单"退出"、驻留提示"退出程序"与窗口关闭退出共用。
//!
//! Windows（尤其 Win7）上托盘菜单回调运行在 Shell 隐藏 tray 窗口的
//! TrackPopupMenu 栈内；即使托盘图标已消失，优雅退出后该栈与消息泵仍可能
//! 驻留，进程无法真正结束。TerminateProcess 是唯一可靠的进程边界：不等待
//! winit 栈回卷、TLS 析构或游离工作线程汇合，OS 一次回收 tray 消息窗口、
//! 全局热键队列与未汇合线程。其他平台没有原生 tray 消息窗口，走优雅退出。

pub fn request_exit() {
    #[cfg(windows)]
    {
        unsafe {
            use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
            let process = GetCurrentProcess();
            if TerminateProcess(process, 0) == 0 {
                // 对当前进程伪句柄几乎不可达；被加固宿主拒绝时退回 CRT 边界。
                std::process::exit(0);
            }
        }
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
