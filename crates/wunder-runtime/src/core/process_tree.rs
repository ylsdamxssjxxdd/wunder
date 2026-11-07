//! 子进程树终止原语。
//!
//! tokio/std 的 `kill` 只终止直接子进程。命令工具拉起的是 shell
//! （pwsh/bash），真正的命令往往是它的子进程：只杀 shell 会留下持有
//! stdout/stderr 管道写端的孤儿，读任务永远等不到 EOF，工具调用随之
//! 卡死；孤儿占着的文件句柄也会让后续文件工具撞上共享冲突。
//!
//! 这里提供按树终止的能力：Windows 把子进程挂进 Job Object 后用
//! `TerminateJobObject` 整树终止（XP 起可用，覆盖 32 位 Win7）；Unix
//! 在 spawn 时建立独立进程组（`process_group(0)`），用 `kill(-pgid)`
//! 整组终止。两者都只作为显式的终止动作，不改变进程的自然生命周期：
//! 命令正常退出后故意脱离的守护进程（如 `Start-Process` 拉起的服务）
//! 不受影响。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::process::Child;

/// 管道 EOF 的宽限窗口。进程退出（或被树终止）后，读任务应在这个窗口内
/// 读到 EOF；超时说明还有进程持有管道写端，需要强制终止整棵树。
pub(crate) const PIPE_EOF_GRACE: Duration = Duration::from_secs(3);

#[cfg(windows)]
mod ffi {
    pub type Handle = *mut core::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateJobObjectW(
            lp_job_attributes: *const core::ffi::c_void,
            lp_name: *const u16,
        ) -> Handle;
        pub fn AssignProcessToJobObject(h_job: Handle, h_process: Handle) -> i32;
        pub fn TerminateJobObject(h_job: Handle, u_exit_code: u32) -> i32;
        pub fn CloseHandle(h_object: Handle) -> i32;
    }
}

/// kernel32 句柄只是进程级值，无线程亲和性；跨 `.await` 移动是安全的。
#[cfg(windows)]
struct JobHandle(ffi::Handle);

#[cfg(windows)]
unsafe impl Send for JobHandle {}
#[cfg(windows)]
unsafe impl Sync for JobHandle {}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        // 句柄只是收尾；未启用 KILL_ON_JOB_CLOSE，关闭不会终止任何进程。
        unsafe { ffi::CloseHandle(self.0) };
    }
}

/// 一条命令子进程的可终止树。`attach` 失败时退化为空守卫，调用方保留
/// 逐进程 kill 作为兜底（与历史行为一致）。
///
/// `Drop` 是最后一道保险：外层看门狗超时、会话级取消等路径会直接丢弃
/// 工具 future，`run_spawned_child_streaming` / detached watcher 的收尾
/// 代码根本不会执行。只要守卫仍 armed，就整树终止；正常收尾必须先
/// `disarm`，否则会误杀命令故意脱管的守护进程。
pub(crate) struct ProcessTree {
    #[cfg(windows)]
    job: Option<JobHandle>,
    #[cfg(unix)]
    pgid: Option<i32>,
    #[cfg(not(any(windows, unix)))]
    _private: (),
    armed: AtomicBool,
}

impl ProcessTree {
    /// spawn 成功后立刻挂载：Windows 把子进程加入新建 Job（孙进程随创建
    /// 自动入 Job）；Unix 记录进程组号（子进程已是组长，pgid == pid）。
    /// 必须赶在子进程派生自己的子进程之前完成，越早挂载覆盖越完整。
    pub(crate) fn attach(child: &Child) -> Self {
        #[cfg(windows)]
        {
            let job = Self::attach_windows_job(child);
            ProcessTree {
                job,
                armed: AtomicBool::new(true),
            }
        }
        #[cfg(unix)]
        {
            let pgid = child.id().map(|pid| pid as i32);
            ProcessTree {
                pgid,
                armed: AtomicBool::new(pgid.is_some()),
            }
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = child;
            ProcessTree {
                _private: (),
                armed: AtomicBool::new(false),
            }
        }
    }

    #[cfg(windows)]
    fn attach_windows_job(child: &Child) -> Option<JobHandle> {
        unsafe {
            let job = ffi::CreateJobObjectW(core::ptr::null(), core::ptr::null());
            if job.is_null() {
                tracing::warn!(
                    "process tree: CreateJobObject failed; falling back to single-process kill"
                );
                return None;
            }
            let handle = JobHandle(job);
            // 桌面版自身若已处于别的 Job（Win7 单 Job 限制），指派会失败；
            // 此时退回逐进程 kill，不视为致命，但必须留下痕迹：树终止会
            // 静默退化为只杀 shell，孙进程仍然存活。
            let Some(process) = child.raw_handle() else {
                tracing::warn!("process tree: child handle unavailable; tree kill disabled");
                return None;
            };
            if ffi::AssignProcessToJobObject(handle.0, process) == 0 {
                tracing::warn!(
                    "process tree: AssignProcessToJobObject failed (parent may already own a job); grandchildren will not be tree-killed"
                );
                return None;
            }
            Some(handle)
        }
    }

    /// 终止整棵命令树（含孙进程）。进程已退出或守卫为空时是无害的空操作。
    pub(crate) fn terminate(&self) {
        // 只生效一次：正常退出后的收尾不该再对复用 pid / 新进程组动手。
        if !self.armed.swap(false, Ordering::Relaxed) {
            return;
        }
        #[cfg(windows)]
        {
            if let Some(job) = self.job.as_ref() {
                unsafe { ffi::TerminateJobObject(job.0, 1) };
            }
        }
        #[cfg(unix)]
        {
            if let Some(pgid) = self.pgid {
                unsafe { kill(-pgid, SIGKILL) };
            }
        }
    }

    /// 正常收尾后解除兜底。调用方确认子进程已退出、输出已收干时调用，
    /// 避免 Drop 对已经消亡的 pid / 进程组再次终止。
    pub(crate) fn disarm(&self) {
        self.armed.store(false, Ordering::Relaxed);
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if self.armed.load(Ordering::Relaxed) {
            // 收尾代码没有跑到（超时、取消、panic、future 被丢弃）：
            // 整树终止，避免孙进程带着管道写端继续存活。
            self.terminate();
        }
    }
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;

    /// `terminate` 必须连孙进程一起收掉：shell 退出后由孙进程持有管道，
    /// 这正是命令工具挂死的根源场景。
    #[tokio::test]
    async fn unix_tree_terminate_kills_grandchildren() {
        use tokio::io::AsyncReadExt;
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg("sleep 300 & echo started");
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
        {
            use std::os::unix::process::CommandExt;
            cmd.as_std_mut().process_group(0);
        }
        let mut child = cmd.spawn().unwrap();
        let tree = ProcessTree::attach(&child);
        let mut stdout = child.stdout.take().unwrap();
        let mut buffer = [0u8; 64];
        let read = stdout.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        tree.terminate();
        // 树终止后孙进程释放管道写端，读到 EOF，read 不再悬挂。
        let mut tail = Vec::new();
        tokio::time::timeout(PIPE_EOF_GRACE, stdout.read_to_end(&mut tail))
            .await
            .expect("pipe must reach EOF after tree terminate")
            .unwrap();
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::process::Stdio;

    /// Job Object 路径必须连孙进程一起收掉：`cmd.exe` 退出后，`start /b`
    /// 拉起的 ping 仍持有 stdout 写端，只杀 shell 会让 join EOF 永久悬挂。
    #[tokio::test]
    async fn windows_job_terminate_kills_grandchildren() {
        use tokio::io::AsyncReadExt;
        let mut cmd = tokio::process::Command::new("cmd.exe");
        cmd.arg("/D")
            .arg("/C")
            .arg("start /b ping -n 30 127.0.0.1 & echo started");
        cmd.stdout(Stdio::piped()).stderr(Stdio::null());
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd.spawn().unwrap();
        let tree = ProcessTree::attach(&child);
        if tree.job.is_none() {
            // 宿主自己可能已经在一个 Job 里（Win7 单 Job 限制），此时树终止
            // 本来就退化为逐进程 kill，不是本用例要验证的路径。
            let _ = child.kill().await;
            let _ = child.wait().await;
            return;
        }
        let mut stdout = child.stdout.take().unwrap();
        let mut buffer = [0u8; 64];
        let read = stdout.read(&mut buffer).await.unwrap();
        assert!(read > 0);
        tree.terminate();
        let mut tail = Vec::new();
        tokio::time::timeout(PIPE_EOF_GRACE, stdout.read_to_end(&mut tail))
            .await
            .expect("pipe must reach EOF after job terminate")
            .unwrap();
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}
