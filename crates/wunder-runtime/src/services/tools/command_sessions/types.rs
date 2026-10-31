use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandSessionStatus {
    Running,
    FailedToStart,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandSessionLaunchMode {
    Direct,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandSessionStream {
    Pty,
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandSessionStartSpec {
    pub command_session_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub user_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub command_index: usize,
    pub command: String,
    pub cwd: String,
    pub shell: Option<String>,
    pub launch_mode: CommandSessionLaunchMode,
    pub tty: bool,
    pub interactive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CommandSessionSnapshot {
    pub command_session_id: String,
    pub tool_call_id: Option<String>,
    pub user_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub command_index: usize,
    pub command: String,
    pub cwd: String,
    pub shell: Option<String>,
    pub launch_mode: CommandSessionLaunchMode,
    pub tty: bool,
    pub interactive: bool,
    pub status: CommandSessionStatus,
    pub seq: u64,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub error: Option<String>,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub pty_bytes: usize,
    pub stdout_dropped_bytes: usize,
    pub stderr_dropped_bytes: usize,
    pub pty_dropped_bytes: usize,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub pty_tail: String,
}

impl CommandSessionSnapshot {
    pub(crate) fn start_event_payload(&self) -> Value {
        let mut map = base_event_map(self);
        map.insert("status".to_string(), json!(self.status));
        Value::Object(map)
    }

    pub(crate) fn status_event_payload(&self) -> Value {
        let mut map = base_event_map(self);
        map.insert("status".to_string(), json!(self.status));
        if let Some(error) = self.error.as_ref() {
            map.insert("error".to_string(), Value::String(error.clone()));
        }
        Value::Object(map)
    }

    pub(crate) fn exit_event_payload(&self) -> Value {
        let mut map = base_event_map(self);
        map.insert("status".to_string(), json!(self.status));
        map.insert("exit_code".to_string(), json!(self.exit_code));
        map.insert("timed_out".to_string(), json!(self.timed_out));
        map.insert("stdout_bytes".to_string(), json!(self.stdout_bytes));
        map.insert("stderr_bytes".to_string(), json!(self.stderr_bytes));
        map.insert("pty_bytes".to_string(), json!(self.pty_bytes));
        map.insert("ended_at".to_string(), json!(self.ended_at));
        if let Some(error) = self.error.as_ref() {
            map.insert("error".to_string(), Value::String(error.clone()));
        }
        if let Some(ended_at) = self.ended_at {
            let duration_ms = ended_at
                .signed_duration_since(self.started_at)
                .num_milliseconds()
                .max(0);
            map.insert("duration_ms".to_string(), json!(duration_ms));
        }
        Value::Object(map)
    }

    pub(crate) fn summary_event_payload(&self) -> Value {
        let mut map = base_event_map(self);
        map.insert("status".to_string(), json!(self.status));
        map.insert("exit_code".to_string(), json!(self.exit_code));
        map.insert("timed_out".to_string(), json!(self.timed_out));
        map.insert("stdout_bytes".to_string(), json!(self.stdout_bytes));
        map.insert("stderr_bytes".to_string(), json!(self.stderr_bytes));
        map.insert("pty_bytes".to_string(), json!(self.pty_bytes));
        map.insert("ended_at".to_string(), json!(self.ended_at));
        if let Some(error) = self.error.as_ref() {
            map.insert("error".to_string(), Value::String(error.clone()));
        }
        if let Some(ended_at) = self.ended_at {
            let duration_ms = ended_at
                .signed_duration_since(self.started_at)
                .num_milliseconds()
                .max(0);
            map.insert("duration_ms".to_string(), json!(duration_ms));
        }
        if self.stdout_dropped_bytes > 0 {
            map.insert(
                "stdout_dropped_bytes".to_string(),
                json!(self.stdout_dropped_bytes),
            );
        }
        if self.stderr_dropped_bytes > 0 {
            map.insert(
                "stderr_dropped_bytes".to_string(),
                json!(self.stderr_dropped_bytes),
            );
        }
        if self.pty_dropped_bytes > 0 {
            map.insert(
                "pty_dropped_bytes".to_string(),
                json!(self.pty_dropped_bytes),
            );
        }
        Value::Object(map)
    }
}

fn base_event_map(snapshot: &CommandSessionSnapshot) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert(
        "command_session_id".to_string(),
        Value::String(snapshot.command_session_id.clone()),
    );
    if let Some(tool_call_id) = snapshot.tool_call_id.as_ref() {
        map.insert(
            "tool_call_id".to_string(),
            Value::String(tool_call_id.clone()),
        );
    }
    map.insert(
        "user_id".to_string(),
        Value::String(snapshot.user_id.clone()),
    );
    map.insert(
        "session_id".to_string(),
        Value::String(snapshot.session_id.clone()),
    );
    map.insert(
        "workspace_id".to_string(),
        Value::String(snapshot.workspace_id.clone()),
    );
    map.insert("command_index".to_string(), json!(snapshot.command_index));
    map.insert(
        "command".to_string(),
        Value::String(snapshot.command.clone()),
    );
    map.insert("cwd".to_string(), Value::String(snapshot.cwd.clone()));
    if let Some(shell) = snapshot.shell.as_ref() {
        map.insert("shell".to_string(), Value::String(shell.clone()));
    }
    map.insert("launch_mode".to_string(), json!(snapshot.launch_mode));
    map.insert("tty".to_string(), json!(snapshot.tty));
    map.insert("interactive".to_string(), json!(snapshot.interactive));
    map.insert("seq".to_string(), json!(snapshot.seq));
    map.insert("started_at".to_string(), json!(snapshot.started_at));
    map.insert("updated_at".to_string(), json!(snapshot.updated_at));
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_output() -> CommandSessionSnapshot {
        let now = Utc::now();
        CommandSessionSnapshot {
            command_session_id: "cmd_test".to_string(),
            tool_call_id: Some("tool_1".to_string()),
            user_id: "user_1".to_string(),
            session_id: "session_1".to_string(),
            workspace_id: "workspace_1".to_string(),
            command_index: 0,
            command: "run".to_string(),
            cwd: "/workspace".to_string(),
            shell: Some("shell".to_string()),
            launch_mode: CommandSessionLaunchMode::Shell,
            tty: false,
            interactive: false,
            status: CommandSessionStatus::Exited,
            seq: 3,
            started_at: now,
            updated_at: now,
            ended_at: Some(now),
            exit_code: Some(0),
            timed_out: false,
            error: None,
            stdout_bytes: 32,
            stderr_bytes: 16,
            pty_bytes: 0,
            stdout_dropped_bytes: 8,
            stderr_dropped_bytes: 0,
            pty_dropped_bytes: 0,
            stdout_tail: "stdout preview".to_string(),
            stderr_tail: "stderr preview".to_string(),
            pty_tail: "pty preview".to_string(),
        }
    }

    #[test]
    fn summary_event_payload_omits_terminal_text() {
        let payload = snapshot_with_output().summary_event_payload();
        let map = payload.as_object().expect("object payload");

        assert!(!map.contains_key("stdout_tail"));
        assert!(!map.contains_key("stderr_tail"));
        assert!(!map.contains_key("pty_tail"));
        assert_eq!(map.get("stdout_bytes"), Some(&json!(32)));
        assert_eq!(map.get("stderr_bytes"), Some(&json!(16)));
        assert_eq!(map.get("stdout_dropped_bytes"), Some(&json!(8)));
    }
}

/// 一条「正在执行的命令任务」的轻量投影。给本地 UI 的后台任务面板用：
/// 只带一眼能读的字段，输出尾部要按需另取，避免每秒轮询时搬运大字符串。
#[derive(Debug, Clone, Serialize)]
pub struct CommandTaskView {
    pub id: String,
    /// 命令行原文（单行化，已按 UI 预算截断）。
    pub command: String,
    pub cwd: String,
    /// 距离开始已运行的毫秒数；已完成的任务为总耗时。
    pub elapsed_ms: i64,
    /// 是否仍在运行。
    pub running: bool,
    pub exit_code: Option<i32>,
    /// `running` / `exited` / `failed_to_start`。
    pub status: &'static str,
    /// 交互式终端会话：终止等同于结束该进程。
    pub tty: bool,
}

/// 展开一条任务时读取的输出细节。三个流分开给，UI 自己决定展示哪个。
#[derive(Debug, Clone, Serialize, Default)]
pub struct CommandTaskDetail {
    pub id: String,
    pub cwd: String,
    pub shell: Option<String>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub error: Option<String>,
    /// 输出是否因为环形缓冲被丢弃过（尾部仍是完整的最后一段）。
    pub truncated: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub pty_tail: String,
}

impl CommandSessionSnapshot {
    /// 轻量投影。命令原文压成一行并截断到 240 字符，够 UI 认出是哪条命令。
    pub(crate) fn task_view(&self, now: DateTime<Utc>) -> CommandTaskView {
        let ended = self.ended_at.unwrap_or(now);
        let elapsed = ended
            .signed_duration_since(self.started_at)
            .num_milliseconds()
            .max(0);
        let single_line = self
            .command
            .split(['\n', '\r'])
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let command = if single_line.chars().count() > 240 {
            let head: String = single_line.chars().take(239).collect();
            format!("{head}…")
        } else {
            single_line
        };
        CommandTaskView {
            id: self.command_session_id.clone(),
            command,
            cwd: self.cwd.clone(),
            elapsed_ms: elapsed,
            running: self.status == CommandSessionStatus::Running,
            exit_code: self.exit_code,
            status: match self.status {
                CommandSessionStatus::Running => "running",
                CommandSessionStatus::Exited => "exited",
                CommandSessionStatus::FailedToStart => "failed_to_start",
            },
            tty: self.tty,
        }
    }

    pub(crate) fn task_detail(&self) -> CommandTaskDetail {
        CommandTaskDetail {
            id: self.command_session_id.clone(),
            cwd: self.cwd.clone(),
            shell: self.shell.clone(),
            exit_code: self.exit_code,
            timed_out: self.timed_out,
            error: self.error.clone(),
            truncated: self.stdout_dropped_bytes > 0
                || self.stderr_dropped_bytes > 0
                || self.pty_dropped_bytes > 0,
            stdout_tail: self.stdout_tail.clone(),
            stderr_tail: self.stderr_tail.clone(),
            pty_tail: self.pty_tail.clone(),
        }
    }
}
