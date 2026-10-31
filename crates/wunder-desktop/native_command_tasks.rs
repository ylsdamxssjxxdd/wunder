//! 命令任务的本地 façade：把运行时里「正在执行的命令」投影给蛹的后台任务面板。
//!
//! 数据来自 `AppState.control.command_sessions`（每条命令工具调用都会开一个
//! 命令会话），所以这里既不轮询子进程、也不重新实现一套任务表：
//! - `running_command_tasks` 只返回仍在运行的任务，字段都是轻量投影；
//! - `command_task_detail` 按需读取输出尾部（展开时才调用）；
//! - `terminate_command_task` 只取消这一条命令的 CancellationToken。
//!
//! 三个方法都是内存操作（DashMap 遍历 + 字符串克隆），调用方可以按秒轮询。

use wunder_server::{CommandTaskDetail, CommandTaskView};

use crate::NativeDesktop;

/// 一条正在执行的命令任务。
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCommandTask {
    pub id: String,
    /// 单行化并截断后的命令行原文。
    pub command: String,
    pub cwd: String,
    pub elapsed_ms: i64,
    pub running: bool,
    pub exit_code: Option<i32>,
    /// `running` / `exited` / `failed_to_start`。
    pub status: String,
    pub tty: bool,
}

/// 展开一条任务时的输出细节。
#[derive(Debug, Clone, PartialEq)]
pub struct NativeCommandTaskDetail {
    pub id: String,
    pub cwd: String,
    pub shell: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub error: Option<String>,
    pub truncated: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub pty_tail: String,
}

fn task_view(task: CommandTaskView) -> NativeCommandTask {
    NativeCommandTask {
        id: task.id,
        command: task.command,
        cwd: task.cwd,
        elapsed_ms: task.elapsed_ms,
        running: task.running,
        exit_code: task.exit_code,
        status: task.status.to_string(),
        tty: task.tty,
    }
}

fn task_detail(detail: CommandTaskDetail) -> NativeCommandTaskDetail {
    NativeCommandTaskDetail {
        id: detail.id,
        cwd: detail.cwd,
        shell: detail.shell,
        exit_code: detail.exit_code,
        timed_out: detail.timed_out,
        error: detail.error,
        truncated: detail.truncated,
        stdout_tail: detail.stdout_tail,
        stderr_tail: detail.stderr_tail,
        pty_tail: detail.pty_tail,
    }
}

impl NativeDesktop {
    /// 当前线程里仍在执行的命令任务，最早的排在前面。
    pub fn running_command_tasks(&self, session_id: &str) -> Vec<NativeCommandTask> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            return Vec::new();
        }
        self.state()
            .control
            .command_sessions
            .running_tasks(self.user_id(), session_id)
            .into_iter()
            .map(task_view)
            .collect()
    }

    /// 展开一条任务时读取它的输出尾部。作用域不匹配时返回 `None`。
    pub fn command_task_detail(
        &self,
        session_id: &str,
        command_session_id: &str,
    ) -> Option<NativeCommandTaskDetail> {
        let (session_id, command_session_id) = (session_id.trim(), command_session_id.trim());
        if session_id.is_empty() || command_session_id.is_empty() {
            return None;
        }
        self.state()
            .control
            .command_sessions
            .task_detail(self.user_id(), session_id, command_session_id)
            .map(task_detail)
    }

    /// 终止一条命令任务。返回 false 表示它已经不在运行或不属于当前线程。
    pub fn terminate_command_task(&self, session_id: &str, command_session_id: &str) -> bool {
        let (session_id, command_session_id) = (session_id.trim(), command_session_id.trim());
        if session_id.is_empty() || command_session_id.is_empty() {
            return false;
        }
        self.state().control.command_sessions.terminate_task(
            self.user_id(),
            session_id,
            command_session_id,
        )
    }
}
