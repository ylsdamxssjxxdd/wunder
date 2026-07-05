use super::types::{
    CommandSessionSnapshot, CommandSessionStartSpec, CommandSessionStatus, CommandSessionStream,
};
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_SESSION_RING_BUFFER_BYTES: usize = 256 * 1024;
const FINISHED_SESSION_RETENTION_MINUTES: i64 = 5;
const SESSION_RING_HEAD_RATIO_NUMERATOR: usize = 3;
const SESSION_RING_HEAD_RATIO_DENOMINATOR: usize = 8;

#[derive(Default)]
struct OutputTailState {
    total_bytes: usize,
    dropped_bytes: usize,
    truncated: bool,
    full_bytes: Vec<u8>,
    head_bytes: Vec<u8>,
    tail_bytes: VecDeque<u8>,
}

impl OutputTailState {
    fn push(&mut self, chunk: &[u8], limit: usize) {
        if chunk.is_empty() {
            return;
        }
        self.total_bytes = self.total_bytes.saturating_add(chunk.len());
        if limit == 0 {
            self.dropped_bytes = self.total_bytes;
            return;
        }

        let (head_limit, tail_limit) = preview_budgets(limit);
        if !self.truncated {
            if self.full_bytes.len().saturating_add(chunk.len()) <= limit {
                self.full_bytes.extend_from_slice(chunk);
                return;
            }
            self.truncated = true;
            let keep_head = head_limit.min(self.full_bytes.len());
            self.head_bytes
                .extend_from_slice(&self.full_bytes[..keep_head]);
            let carry_tail = self.full_bytes[keep_head..].to_vec();
            self.full_bytes.clear();
            self.push_tail_bytes(&carry_tail, tail_limit);
        }

        let mut remaining = chunk;
        if self.head_bytes.len() < head_limit {
            let missing = head_limit - self.head_bytes.len();
            let take = missing.min(remaining.len());
            self.head_bytes.extend_from_slice(&remaining[..take]);
            remaining = &remaining[take..];
        }
        self.push_tail_bytes(remaining, tail_limit);
        self.dropped_bytes = self.total_bytes.saturating_sub(self.kept_bytes());
    }

    fn text(&self) -> String {
        if !self.truncated {
            if self.full_bytes.is_empty() {
                return String::new();
            }
            return String::from_utf8_lossy(&self.full_bytes).into_owned();
        }

        let head = String::from_utf8_lossy(&self.head_bytes).into_owned();
        let tail_bytes = self.tail_bytes.iter().copied().collect::<Vec<_>>();
        let tail = String::from_utf8_lossy(&tail_bytes).into_owned();
        let marker = format!(
            "...(truncated command output, omitted {} bytes)...",
            self.dropped_bytes
        );
        match (head.is_empty(), tail.is_empty()) {
            (true, true) => marker,
            (true, false) => format!("{marker}\n{tail}"),
            (false, true) => format!("{head}\n{marker}"),
            (false, false) => format!("{head}\n{marker}\n{tail}"),
        }
    }

    fn kept_bytes(&self) -> usize {
        if self.truncated {
            self.head_bytes.len().saturating_add(self.tail_bytes.len())
        } else {
            self.full_bytes.len()
        }
    }

    fn push_tail_bytes(&mut self, bytes: &[u8], limit: usize) {
        if bytes.is_empty() || limit == 0 {
            return;
        }
        if bytes.len() >= limit {
            self.tail_bytes.clear();
            self.tail_bytes
                .extend(bytes[bytes.len().saturating_sub(limit)..].iter().copied());
            return;
        }
        let overflow = self.tail_bytes.len().saturating_add(bytes.len());
        if overflow > limit {
            self.tail_bytes.drain(..overflow - limit);
        }
        self.tail_bytes.extend(bytes.iter().copied());
    }
}

fn preview_budgets(limit: usize) -> (usize, usize) {
    if limit <= 1 {
        return (limit, 0);
    }
    let head = ((limit as u128 * SESSION_RING_HEAD_RATIO_NUMERATOR as u128)
        / SESSION_RING_HEAD_RATIO_DENOMINATOR as u128) as usize;
    let head = head.clamp(1, limit - 1);
    (head, limit - head)
}

struct CommandSessionRecord {
    command_session_id: String,
    tool_call_id: Option<String>,
    user_id: String,
    session_id: String,
    workspace_id: String,
    command_index: usize,
    command: String,
    cwd: String,
    shell: Option<String>,
    launch_mode: super::types::CommandSessionLaunchMode,
    tty: bool,
    interactive: bool,
    status: CommandSessionStatus,
    seq: u64,
    started_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    exit_code: Option<i32>,
    timed_out: bool,
    error: Option<String>,
    stdout: OutputTailState,
    stderr: OutputTailState,
    pty: OutputTailState,
}

impl CommandSessionRecord {
    fn from_start_spec(spec: CommandSessionStartSpec, command_session_id: String) -> Self {
        let now = Utc::now();
        Self {
            command_session_id,
            tool_call_id: spec.tool_call_id,
            user_id: spec.user_id,
            session_id: spec.session_id,
            workspace_id: spec.workspace_id,
            command_index: spec.command_index,
            command: spec.command,
            cwd: spec.cwd,
            shell: spec.shell,
            launch_mode: spec.launch_mode,
            tty: spec.tty,
            interactive: spec.interactive,
            status: CommandSessionStatus::Running,
            seq: 0,
            started_at: now,
            updated_at: now,
            ended_at: None,
            expires_at: None,
            exit_code: None,
            timed_out: false,
            error: None,
            stdout: OutputTailState::default(),
            stderr: OutputTailState::default(),
            pty: OutputTailState::default(),
        }
    }

    fn stream_mut(&mut self, stream: CommandSessionStream) -> &mut OutputTailState {
        match stream {
            CommandSessionStream::Pty => &mut self.pty,
            CommandSessionStream::Stdout => &mut self.stdout,
            CommandSessionStream::Stderr => &mut self.stderr,
        }
    }

    fn snapshot(&self) -> CommandSessionSnapshot {
        CommandSessionSnapshot {
            command_session_id: self.command_session_id.clone(),
            tool_call_id: self.tool_call_id.clone(),
            user_id: self.user_id.clone(),
            session_id: self.session_id.clone(),
            workspace_id: self.workspace_id.clone(),
            command_index: self.command_index,
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            shell: self.shell.clone(),
            launch_mode: self.launch_mode,
            tty: self.tty,
            interactive: self.interactive,
            status: self.status,
            seq: self.seq,
            started_at: self.started_at,
            updated_at: self.updated_at,
            ended_at: self.ended_at,
            exit_code: self.exit_code,
            timed_out: self.timed_out,
            error: self.error.clone(),
            stdout_bytes: self.stdout.total_bytes,
            stderr_bytes: self.stderr.total_bytes,
            pty_bytes: self.pty.total_bytes,
            stdout_dropped_bytes: self.stdout.dropped_bytes,
            stderr_dropped_bytes: self.stderr.dropped_bytes,
            pty_dropped_bytes: self.pty.dropped_bytes,
            stdout_tail: self.stdout.text(),
            stderr_tail: self.stderr.text(),
            pty_tail: self.pty.text(),
        }
    }
}

#[derive(Default)]
pub struct CommandSessionBroker {
    sessions: DashMap<String, Arc<Mutex<CommandSessionRecord>>>,
    ring_buffer_bytes: usize,
}

impl CommandSessionBroker {
    pub(crate) fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            ring_buffer_bytes: DEFAULT_SESSION_RING_BUFFER_BYTES,
        }
    }

    pub(crate) fn generate_session_id() -> String {
        format!("cmd_{}", Uuid::new_v4().simple())
    }

    pub(crate) fn start_session(&self, spec: CommandSessionStartSpec) -> CommandSessionSnapshot {
        self.prune_expired();
        let command_session_id = spec
            .command_session_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(Self::generate_session_id);
        let record = Arc::new(Mutex::new(CommandSessionRecord::from_start_spec(
            spec,
            command_session_id.clone(),
        )));
        let snapshot = record.lock().snapshot();
        self.sessions.insert(command_session_id, record);
        snapshot
    }

    pub(crate) fn append_delta(
        &self,
        command_session_id: &str,
        stream: CommandSessionStream,
        chunk: &[u8],
    ) -> Option<u64> {
        if chunk.is_empty() {
            return None;
        }
        let entry = self.sessions.get(command_session_id)?;
        let mut record = entry.value().lock();
        record.seq = record.seq.saturating_add(1);
        record.updated_at = Utc::now();
        record
            .stream_mut(stream)
            .push(chunk, self.ring_buffer_bytes);
        Some(record.seq)
    }

    pub(crate) fn mark_failed_to_start(
        &self,
        command_session_id: &str,
        error: impl Into<String>,
    ) -> Option<CommandSessionSnapshot> {
        let entry = self.sessions.get(command_session_id)?;
        let mut record = entry.value().lock();
        record.seq = record.seq.saturating_add(1);
        record.status = CommandSessionStatus::FailedToStart;
        record.updated_at = Utc::now();
        record.ended_at = Some(record.updated_at);
        record.expires_at =
            Some(record.updated_at + Duration::minutes(FINISHED_SESSION_RETENTION_MINUTES));
        record.error = Some(error.into());
        Some(record.snapshot())
    }

    pub(crate) fn finish_session(
        &self,
        command_session_id: &str,
        exit_code: Option<i32>,
        timed_out: bool,
        error: Option<String>,
    ) -> Option<CommandSessionSnapshot> {
        let entry = self.sessions.get(command_session_id)?;
        let mut record = entry.value().lock();
        record.seq = record.seq.saturating_add(1);
        record.status = CommandSessionStatus::Exited;
        record.updated_at = Utc::now();
        record.ended_at = Some(record.updated_at);
        record.expires_at =
            Some(record.updated_at + Duration::minutes(FINISHED_SESSION_RETENTION_MINUTES));
        record.exit_code = exit_code;
        record.timed_out = timed_out;
        record.error = error;
        Some(record.snapshot())
    }

    pub(crate) fn snapshot(&self, command_session_id: &str) -> Option<CommandSessionSnapshot> {
        self.prune_expired();
        let entry = self.sessions.get(command_session_id)?;
        let snapshot = entry.value().lock().snapshot();
        Some(snapshot)
    }

    pub(crate) fn snapshot_for_scope(
        &self,
        user_id: &str,
        session_id: &str,
        command_session_id: &str,
    ) -> Option<CommandSessionSnapshot> {
        let snapshot = self.snapshot(command_session_id)?;
        if snapshot.user_id != user_id || snapshot.session_id != session_id {
            return None;
        }
        Some(snapshot)
    }

    pub(crate) fn list_session_snapshots(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Vec<CommandSessionSnapshot> {
        self.prune_expired();
        let mut snapshots = self
            .sessions
            .iter()
            .filter_map(|entry| {
                let snapshot = entry.value().lock().snapshot();
                if snapshot.user_id != user_id || snapshot.session_id != session_id {
                    return None;
                }
                Some(snapshot)
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| {
            left.command_index
                .cmp(&right.command_index)
                .then(left.started_at.cmp(&right.started_at))
                .then(left.command_session_id.cmp(&right.command_session_id))
        });
        snapshots
    }

    fn prune_expired(&self) {
        let now = Utc::now();
        let expired = self
            .sessions
            .iter()
            .filter_map(|entry| {
                let expires_at = entry.value().lock().expires_at;
                expires_at
                    .filter(|deadline| *deadline <= now)
                    .map(|_| entry.key().clone())
            })
            .collect::<Vec<_>>();
        for key in expired {
            self.sessions.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::tools::command_sessions::types::{
        CommandSessionLaunchMode, CommandSessionStatus, CommandSessionStream,
    };

    fn build_start_spec() -> CommandSessionStartSpec {
        CommandSessionStartSpec {
            command_session_id: Some("cmd_test".to_string()),
            tool_call_id: Some("tool_1".to_string()),
            user_id: "user_a".to_string(),
            session_id: "sess_1".to_string(),
            workspace_id: "ws_1".to_string(),
            command_index: 0,
            command: "echo hi".to_string(),
            cwd: "/tmp".to_string(),
            shell: Some("bash".to_string()),
            launch_mode: CommandSessionLaunchMode::Shell,
            tty: false,
            interactive: false,
        }
    }

    #[test]
    fn broker_keeps_head_and_tail_preview_per_stream() {
        let broker = CommandSessionBroker::new();
        let snapshot = broker.start_session(build_start_spec());
        assert_eq!(snapshot.command_session_id, "cmd_test");

        let chunk = format!(
            "{}{}{}",
            "a".repeat(DEFAULT_SESSION_RING_BUFFER_BYTES / 2),
            "b".repeat(32),
            "z".repeat(DEFAULT_SESSION_RING_BUFFER_BYTES / 2)
        )
        .into_bytes();
        let seq = broker
            .append_delta("cmd_test", CommandSessionStream::Stdout, &chunk)
            .expect("seq");
        assert_eq!(seq, 1);

        let snapshot = broker.snapshot("cmd_test").expect("snapshot");
        assert_eq!(snapshot.stdout_bytes, chunk.len());
        assert_eq!(snapshot.stdout_dropped_bytes, 32);
        assert!(snapshot.stdout_tail.starts_with('a'));
        assert!(snapshot.stdout_tail.contains("omitted 32 bytes"));
        assert!(snapshot.stdout_tail.ends_with('z'));
    }

    #[test]
    fn broker_marks_exit_and_preserves_summary() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        broker.append_delta("cmd_test", CommandSessionStream::Stdout, b"alpha\n");
        broker.append_delta("cmd_test", CommandSessionStream::Stderr, b"beta\n");

        let snapshot = broker
            .finish_session("cmd_test", Some(0), false, None)
            .expect("finished");

        assert_eq!(snapshot.status, CommandSessionStatus::Exited);
        assert_eq!(snapshot.exit_code, Some(0));
        assert!(snapshot.stdout_tail.contains("alpha"));
        assert!(snapshot.stderr_tail.contains("beta"));
    }

    #[test]
    fn broker_lists_snapshots_only_for_matching_scope() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        broker.start_session(CommandSessionStartSpec {
            command_session_id: Some("cmd_other".to_string()),
            tool_call_id: Some("tool_2".to_string()),
            user_id: "user_b".to_string(),
            session_id: "sess_2".to_string(),
            workspace_id: "ws_2".to_string(),
            command_index: 1,
            command: "pwd".to_string(),
            cwd: "/srv".to_string(),
            shell: Some("bash".to_string()),
            launch_mode: CommandSessionLaunchMode::Shell,
            tty: false,
            interactive: false,
        });

        let scoped = broker.list_session_snapshots("user_a", "sess_1");
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].command_session_id, "cmd_test");
        assert!(broker
            .snapshot_for_scope("user_a", "sess_1", "cmd_test")
            .is_some());
        assert!(broker
            .snapshot_for_scope("user_a", "sess_1", "cmd_other")
            .is_none());
    }
}
