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

#[derive(Debug, Clone)]
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
        if !self.pty_tail.is_empty() {
            map.insert("pty_tail".to_string(), Value::String(self.pty_tail.clone()));
        }
        if !self.stdout_tail.is_empty() {
            map.insert("stdout_tail".to_string(), Value::String(self.stdout_tail.clone()));
        }
        if !self.stderr_tail.is_empty() {
            map.insert("stderr_tail".to_string(), Value::String(self.stderr_tail.clone()));
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
            map.insert("pty_dropped_bytes".to_string(), json!(self.pty_dropped_bytes));
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
        map.insert("tool_call_id".to_string(), Value::String(tool_call_id.clone()));
    }
    map.insert("user_id".to_string(), Value::String(snapshot.user_id.clone()));
    map.insert(
        "session_id".to_string(),
        Value::String(snapshot.session_id.clone()),
    );
    map.insert(
        "workspace_id".to_string(),
        Value::String(snapshot.workspace_id.clone()),
    );
    map.insert(
        "command_index".to_string(),
        json!(snapshot.command_index),
    );
    map.insert("command".to_string(), Value::String(snapshot.command.clone()));
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
