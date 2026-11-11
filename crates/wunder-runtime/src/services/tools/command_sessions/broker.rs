use super::types::{
    CommandSessionSnapshot, CommandSessionStartSpec, CommandSessionStatus, CommandSessionStream,
    CommandTaskDetail, CommandTaskView,
};
use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const DEFAULT_SESSION_RING_BUFFER_BYTES: usize = 256 * 1024;
const FINISHED_SESSION_RETENTION_MINUTES: i64 = 5;
const SESSION_RING_HEAD_RATIO_NUMERATOR: usize = 3;
const SESSION_RING_HEAD_RATIO_DENOMINATOR: usize = 8;
const MAX_ACTIVE_COMMAND_PROCESSES: usize = 16;
const MAX_RETAINED_DELTAS: usize = 4096;

#[derive(Clone)]
pub(crate) struct CommandSessionEvent {
    pub user_id: String,
    pub session_id: String,
    pub event_type: &'static str,
    pub data: serde_json::Value,
}

struct OutputDelta {
    seq: u64,
    stream: CommandSessionStream,
    text: String,
}

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
        // Command output is decoded from the *complete* byte buffer, so a
        // Windows GBK code page is recovered instead of being destroyed by a
        // lossy UTF-8 conversion. Streaming deltas use the same decoder.
        if !self.truncated {
            if self.full_bytes.is_empty() {
                return String::new();
            }
            return crate::tools::decode_command_output(&self.full_bytes);
        }

        let head = crate::tools::decode_command_output(&self.head_bytes);
        let tail_bytes = self.tail_bytes.iter().copied().collect::<Vec<_>>();
        let tail = crate::tools::decode_command_output(&tail_bytes);
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
    deltas: VecDeque<OutputDelta>,
    delta_bytes: usize,
    model_cursor: u64,
    dropped_through_seq: u64,
    changed: Arc<Notify>,
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
            deltas: VecDeque::new(),
            delta_bytes: 0,
            model_cursor: 0,
            dropped_through_seq: 0,
            changed: Arc::new(Notify::new()),
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

pub struct CommandSessionBroker {
    sessions: DashMap<String, Arc<Mutex<CommandSessionRecord>>>,
    processes: DashMap<String, Arc<CommandProcessHandle>>,
    /// 前台（阻塞式）命令的会话级取消令牌。detached 命令有自己的进程句柄，
    /// 前台命令没有，「终止这一条」就落到这张表上，由执行中的命令循环消费。
    cancels: DashMap<String, CancellationToken>,
    ring_buffer_bytes: usize,
    active_processes: AtomicUsize,
    events: tokio::sync::broadcast::Sender<CommandSessionEvent>,
}

/// Runtime-only controls for a detached command. The durable/session snapshot
/// remains separate so polling never needs to hold a child-process lock.
pub(crate) struct CommandProcessHandle {
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    cancel: CancellationToken,
}

/// 一次 stdin 写入的结果。
///
/// 这两种「没写进去」都不是运行时故障，调用方要能区分并继续读最终输出：
/// 命令跑完以后再写输入是正常时序，不能因此丢掉它已经产出的输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandStdinWrite {
    /// 输入已经交给进程。
    Written,
    /// 会话没有可写的 stdin（进程不是以可交互方式启动，或 stdin 已关闭）。
    StdinClosed,
    /// 会话已经结束，本次输入没有生效。
    SessionExited,
}

impl CommandProcessHandle {
    pub(crate) fn new(stdin: Option<ChildStdin>) -> Self {
        Self {
            stdin: tokio::sync::Mutex::new(stdin),
            cancel: CancellationToken::new(),
        }
    }

    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub(crate) fn stdin_writable(&self) -> bool {
        self.stdin
            .try_lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    pub(crate) async fn write_stdin(&self, input: &[u8]) -> Result<CommandStdinWrite, String> {
        let mut guard = self.stdin.lock().await;
        let Some(stdin) = guard.as_mut() else {
            return Ok(CommandStdinWrite::StdinClosed);
        };
        if let Err(error) = stdin.write_all(input).await {
            return Err(format!("failed to write command stdin: {error}"));
        }
        // 很多交互式程序读完一行才动起来，不 flush 会让模型以为输入没生效。
        if let Err(error) = stdin.flush().await {
            return Err(format!("failed to flush command stdin: {error}"));
        }
        Ok(CommandStdinWrite::Written)
    }

    pub(crate) fn cancel(&self) {
        self.cancel.cancel();
    }
}

impl Default for CommandSessionBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandSessionBroker {
    pub(crate) fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            processes: DashMap::new(),
            cancels: DashMap::new(),
            ring_buffer_bytes: DEFAULT_SESSION_RING_BUFFER_BYTES,
            active_processes: AtomicUsize::new(0),
            events: tokio::sync::broadcast::channel(256).0,
        }
    }

    pub(crate) fn generate_session_id() -> String {
        format!("cmd_{}", Uuid::new_v4().simple())
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CommandSessionEvent> {
        self.events.subscribe()
    }

    pub(crate) fn publish_event(
        &self,
        command_session_id: &str,
        event_type: &'static str,
        data: serde_json::Value,
    ) {
        let Some(entry) = self.sessions.get(command_session_id) else {
            return;
        };
        let record = entry.value().lock();
        let _ = self.events.send(CommandSessionEvent {
            user_id: record.user_id.clone(),
            session_id: record.session_id.clone(),
            event_type,
            data,
        });
    }

    pub(crate) fn mark_output_gap(&self, command_session_id: &str) {
        if let Some(entry) = self.sessions.get(command_session_id) {
            let mut record = entry.value().lock();
            // The remote cursor and broker sequence are independent. Advance
            // the local sequence so an unread gap is visible even with no text.
            record.seq = record.seq.saturating_add(1);
            record.dropped_through_seq = record.seq;
            record.changed.notify_waiters();
        }
    }

    pub(crate) fn start_session(&self, spec: CommandSessionStartSpec) -> CommandSessionSnapshot {
        self.prune_expired();
        if let Some(command_session_id) = spec
            .command_session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            if let Some(existing) = self.sessions.get(command_session_id) {
                let snapshot = existing.value().lock().snapshot();
                if snapshot.user_id == spec.user_id && snapshot.session_id == spec.session_id {
                    return snapshot;
                }
            }
        }
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
        let text = crate::tools::decode_command_output(chunk);
        record.delta_bytes = record.delta_bytes.saturating_add(text.len());
        let seq = record.seq;
        record.deltas.push_back(OutputDelta { seq, stream, text });
        while record.delta_bytes > self.ring_buffer_bytes
            || record.deltas.len() > MAX_RETAINED_DELTAS
        {
            if let Some(delta) = record.deltas.pop_front() {
                record.delta_bytes = record.delta_bytes.saturating_sub(delta.text.len());
                record.dropped_through_seq = record.dropped_through_seq.max(delta.seq);
            }
        }
        record.changed.notify_waiters();
        let _ = self.events.send(CommandSessionEvent {
            user_id: record.user_id.clone(),
            session_id: record.session_id.clone(),
            event_type: "command_session_delta",
            data: serde_json::json!({
                "command_session_id": record.command_session_id,
                "tool_call_id": record.tool_call_id,
                "command_index": record.command_index,
                "stream": stream,
                "delta": crate::tools::decode_command_output(chunk), "seq": seq,
            }),
        });
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
        record.changed.notify_waiters();
        let snapshot = record.snapshot();
        let _ = self.events.send(CommandSessionEvent {
            user_id: record.user_id.clone(),
            session_id: record.session_id.clone(),
            event_type: "command_session_status",
            data: snapshot.status_event_payload(),
        });
        Some(snapshot)
    }

    pub(crate) fn finish_session(
        &self,
        command_session_id: &str,
        exit_code: Option<i32>,
        timed_out: bool,
        error: Option<String>,
    ) -> Option<CommandSessionSnapshot> {
        self.finish_session_with(command_session_id, exit_code, timed_out, error, false)
    }

    fn finish_session_with(
        &self,
        command_session_id: &str,
        exit_code: Option<i32>,
        timed_out: bool,
        error: Option<String>,
        only_if_running: bool,
    ) -> Option<CommandSessionSnapshot> {
        let entry = self.sessions.get(command_session_id)?;
        let mut record = entry.value().lock();
        if only_if_running && record.status != CommandSessionStatus::Running {
            return None;
        }
        record.seq = record.seq.saturating_add(1);
        record.status = CommandSessionStatus::Exited;
        record.updated_at = Utc::now();
        record.ended_at = Some(record.updated_at);
        record.expires_at =
            Some(record.updated_at + Duration::minutes(FINISHED_SESSION_RETENTION_MINUTES));
        record.exit_code = exit_code;
        record.timed_out = timed_out;
        record.error = error;
        record.changed.notify_waiters();
        let snapshot = record.snapshot();
        let _ = self.events.send(CommandSessionEvent {
            user_id: record.user_id.clone(),
            session_id: record.session_id.clone(),
            event_type: "command_session_exit",
            data: snapshot.exit_event_payload(),
        });
        Some(snapshot)
    }

    /// 仅当会话仍处于运行中时补一次收尾。用于执行生命周期守卫与终止兜底，
    /// 重复调用不会覆盖已有的退出码或产生重复事件。
    pub(crate) fn finish_session_if_running(
        &self,
        command_session_id: &str,
        error: impl Into<String>,
    ) -> Option<CommandSessionSnapshot> {
        self.finish_session_with(command_session_id, None, false, Some(error.into()), true)
    }

    pub(crate) fn register_process(
        &self,
        command_session_id: &str,
        process: Arc<CommandProcessHandle>,
    ) -> bool {
        if self.snapshot(command_session_id).is_none() {
            return false;
        }
        match self.processes.entry(command_session_id.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                // A retry may re-register the same session. Cancel the stale
                // handle before replacing it so the old child cannot leak.
                let stale = entry.insert(process);
                stale.cancel();
                true
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                // Holding the vacant entry prevents a same-ID concurrent
                // launch from bypassing this capacity reservation.
                let mut current = self.active_processes.load(Ordering::Acquire);
                loop {
                    if current >= MAX_ACTIVE_COMMAND_PROCESSES {
                        return false;
                    }
                    match self.active_processes.compare_exchange_weak(
                        current,
                        current + 1,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(observed) => current = observed,
                    }
                }
                entry.insert(process);
                true
            }
        }
    }

    pub(crate) fn remove_process(&self, command_session_id: &str) {
        if self.processes.remove(command_session_id).is_some() {
            self.active_processes.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// 为一条仍在运行的会话登记取消令牌。与 `processes` 表不同，这里不占
    /// detached 进程名额：前台阻塞命令每个线程同一时刻至多一条，由调用方在
    /// 执行返回时用 `unregister_cancel` 摘除，令牌不会跨命令残留。
    pub(crate) fn register_cancel(&self, command_session_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        if let Some(stale) = self
            .cancels
            .insert(command_session_id.to_string(), token.clone())
        {
            stale.cancel();
        }
        token
    }

    pub(crate) fn unregister_cancel(&self, command_session_id: &str) {
        self.cancels.remove(command_session_id);
    }

    pub(crate) async fn write_stdin(
        &self,
        user_id: &str,
        session_id: &str,
        command_session_id: &str,
        input: &[u8],
    ) -> Result<CommandStdinWrite, String> {
        let snapshot = self
            .snapshot_for_scope(user_id, session_id, command_session_id)
            .ok_or_else(|| "unknown command session".to_string())?;
        // 已经结束的会话不是错误：调用方还要读最后一次输出。
        if snapshot.status != CommandSessionStatus::Running {
            return Ok(CommandStdinWrite::SessionExited);
        }
        let Some(process) = self
            .processes
            .get(command_session_id)
            .map(|entry| Arc::clone(entry.value()))
        else {
            // 退出瞬间快照可能还没落到 exited，进程句柄已释放同样按已退出处理。
            return Ok(CommandStdinWrite::SessionExited);
        };
        process.write_stdin(input).await
    }

    pub(crate) async fn poll(
        &self,
        user_id: &str,
        session_id: &str,
        command_session_id: &str,
        after_seq: Option<u64>,
        yield_time: std::time::Duration,
        wait_for_exit: bool,
    ) -> Result<serde_json::Value, String> {
        // Hold only this command's record, never a DashMap guard across await.
        let record = self
            .sessions
            .get(command_session_id)
            .map(|entry| Arc::clone(entry.value()))
            .ok_or_else(|| "unknown command session".to_string())?;
        let (changed, cursor) = {
            let record = record.lock();
            if record.user_id != user_id || record.session_id != session_id {
                return Err("unknown command session".to_string());
            }
            (
                Arc::clone(&record.changed),
                after_seq.unwrap_or(record.model_cursor),
            )
        };
        let deadline = tokio::time::Instant::now() + yield_time;
        loop {
            let notified = changed.notified();
            tokio::pin!(notified);
            // notify_waiters does not retain a permit: enable before checking.
            notified.as_mut().enable();
            {
                let mut record = record.lock();
                if record.status != CommandSessionStatus::Running
                    || (!wait_for_exit && record.seq > cursor)
                    || tokio::time::Instant::now() >= deadline
                {
                    let mut stdout = String::new();
                    let mut stderr = String::new();
                    let mut pty = String::new();
                    for delta in record.deltas.iter().filter(|delta| delta.seq > cursor) {
                        match delta.stream {
                            CommandSessionStream::Stdout => stdout.push_str(&delta.text),
                            CommandSessionStream::Stderr => stderr.push_str(&delta.text),
                            CommandSessionStream::Pty => pty.push_str(&delta.text),
                        }
                    }
                    let dropped = cursor < record.dropped_through_seq;
                    record.model_cursor = record.model_cursor.max(record.seq);
                    return Ok(serde_json::json!({
                        "command_session_id": record.command_session_id,
                        "status": record.status,
                        "seq": record.seq,
                        "exit_code": record.exit_code,
                        "timed_out": record.timed_out,
                        "error": record.error,
                        "stdout": stdout, "stderr": stderr, "pty": pty,
                        "dropped": dropped,
                        "output_truncated": dropped,
                    }));
                }
            }
            let _ = tokio::time::timeout_at(deadline, notified).await;
        }
    }

    pub(crate) fn terminate_scope(&self, user_id: &str, session_id: &str) -> usize {
        let ids = self
            .sessions
            .iter()
            .filter_map(|entry| {
                let snapshot = entry.value().lock().snapshot();
                (snapshot.user_id == user_id
                    && snapshot.session_id == session_id
                    && snapshot.status == CommandSessionStatus::Running)
                    .then(|| snapshot.command_session_id)
            })
            .collect::<Vec<_>>();
        let mut cancelled = 0;
        for id in ids {
            if let Some(process) = self
                .processes
                .get(&id)
                .map(|entry| Arc::clone(entry.value()))
            {
                // Keep the reservation until the detached watcher observes
                // process exit. Removing it here would permit a short-lived
                // oversubscription while the operating system reaps the child.
                process.cancel();
                cancelled += 1;
            } else if let Some(cancel) = self.cancels.get(&id).map(|entry| entry.value().clone()) {
                // 前台阻塞命令没有进程句柄，靠会话级令牌让执行路径自行
                // 终止子进程并走到正常的 emit_exit。
                cancel.cancel();
                cancelled += 1;
            } else {
                // 既无进程也无取消令牌：执行路径已不在（超时、线程取消），
                // 子进程由 kill_on_drop 回收，这里把运行中的会话收尾，
                // 避免残留僵尸任务。
                self.finish_session_if_running(&id, "command cancelled");
                cancelled += 1;
            }
        }
        cancelled
    }

    pub(crate) fn running_session_ids(&self, user_id: &str, session_id: &str) -> Vec<String> {
        self.sessions
            .iter()
            .filter_map(|entry| {
                let snapshot = entry.value().lock().snapshot();
                (snapshot.user_id == user_id
                    && snapshot.session_id == session_id
                    && snapshot.status == CommandSessionStatus::Running)
                    .then_some(snapshot.command_session_id)
            })
            .collect()
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
        self.prune_expired();
        let entry = self.sessions.get(command_session_id)?;
        let record = entry.value().lock();
        if record.user_id != user_id || record.session_id != session_id {
            return None;
        }
        Some(record.snapshot())
    }

    /// 正在执行的命令任务（本地 UI 的后台任务面板）。已完成的任务不进这里：
    /// 面板的语义是「现在还在跑什么」，历史由时间线与日志承担。
    pub fn running_tasks(&self, user_id: &str, session_id: &str) -> Vec<CommandTaskView> {
        self.prune_expired();
        let now = Utc::now();
        let mut tasks = self
            .sessions
            .iter()
            .filter_map(|entry| {
                let record = entry.value().lock();
                if record.user_id != user_id
                    || record.session_id != session_id
                    || record.status != CommandSessionStatus::Running
                {
                    return None;
                }
                Some(record.snapshot().task_view(now))
            })
            .collect::<Vec<_>>();
        // 启动早的排在前面：用户看到的是「最早那个跑了多久」。
        tasks.sort_by(|left, right| {
            right
                .elapsed_ms
                .cmp(&left.elapsed_ms)
                .then(left.id.cmp(&right.id))
        });
        tasks
    }

    /// 展开一条任务时读取输出尾部。作用域校验和 `snapshot_for_scope` 一致，
    /// 不能靠 id 猜别的会话。
    pub fn task_detail(
        &self,
        user_id: &str,
        session_id: &str,
        command_session_id: &str,
    ) -> Option<CommandTaskDetail> {
        self.prune_expired();
        let entry = self.sessions.get(command_session_id)?;
        let record = entry.value().lock();
        if record.user_id != user_id || record.session_id != session_id {
            return None;
        }
        Some(record.snapshot().task_detail())
    }

    /// 终止单条命令任务。detached 命令取消它自己的进程句柄令牌；前台阻塞命令
    /// 没有进程句柄，取消落到执行时登记的会话级令牌上。两种情况都只影响这一
    /// 条，其余任务与线程本身不受影响；进程回收仍由原来的执行路径完成，
    /// detached 名额在进程真正退出后才释放（与 `terminate_scope` 同一约束）。
    pub fn terminate_task(
        &self,
        user_id: &str,
        session_id: &str,
        command_session_id: &str,
    ) -> bool {
        let in_scope = self
            .sessions
            .get(command_session_id)
            .map(|entry| {
                let record = entry.value().lock();
                record.user_id == user_id
                    && record.session_id == session_id
                    && record.status == CommandSessionStatus::Running
            })
            .unwrap_or(false);
        if !in_scope {
            return false;
        }
        if let Some(process) = self
            .processes
            .get(command_session_id)
            .map(|entry| Arc::clone(entry.value()))
        {
            process.cancel();
            return true;
        }
        if let Some(cancel) = self.cancels.get(command_session_id) {
            cancel.cancel();
            return true;
        }
        // 没有进程句柄也没有取消令牌，说明执行路径已经不在（工具超时、线程
        // 取消或被丢弃的 future）。子进程会由 kill_on_drop 回收，这里直接把
        // 运行中的会话收尾，避免面板里留下永远无法移除的僵尸任务。
        self.finish_session_if_running(command_session_id, "command cancelled")
            .is_some()
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
                let record = entry.value().lock();
                if record.user_id != user_id || record.session_id != session_id {
                    return None;
                }
                Some(record.snapshot())
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

    #[cfg(windows)]
    #[test]
    fn output_tail_state_decodes_gbk_output_for_the_model() {
        let (encoded, _, _) = encoding_rs::GBK.encode("中文输出正常");
        let mut state = OutputTailState::default();
        state.push(&encoded, 4096);
        assert_eq!(state.text(), "中文输出正常");
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

    #[tokio::test]
    async fn poll_returns_updated_exit_snapshot_without_cross_scope_access() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        broker.append_delta("cmd_test", CommandSessionStream::Stdout, b"ready\n");
        broker.finish_session("cmd_test", Some(0), false, None);

        let snapshot = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                true,
            )
            .await
            .expect("scoped snapshot");
        assert_eq!(snapshot["status"], "exited");
        assert_eq!(snapshot["exit_code"], 0);
        assert_eq!(snapshot["stdout"], "ready\n");
        assert!(broker
            .poll(
                "user_b",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                true
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn incremental_poll_keeps_ui_snapshot_and_supports_explicit_replay() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        broker.append_delta("cmd_test", CommandSessionStream::Stdout, b"first\n");
        let first = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        assert_eq!(first["stdout"], "first\n");
        broker.append_delta("cmd_test", CommandSessionStream::Stderr, b"second\n");
        let second = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        assert_eq!(second["stdout"], "");
        assert_eq!(second["stderr"], "second\n");
        let replay = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                Some(0),
                std::time::Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        assert_eq!(replay["stdout"], "first\n");
        assert_eq!(replay["stderr"], "second\n");
        let empty = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        assert_eq!(empty["stdout"], "");
        assert_eq!(empty["stderr"], "");
        assert_eq!(broker.snapshot("cmd_test").unwrap().stdout_tail, "first\n");
    }

    #[tokio::test]
    async fn wait_for_exit_streams_output_without_finishing_model_poll() {
        let broker = Arc::new(CommandSessionBroker::new());
        broker.start_session(build_start_spec());
        let mut events = broker.subscribe();
        let waiting = Arc::clone(&broker);
        let poll = tokio::spawn(async move {
            waiting
                .poll(
                    "user_a",
                    "sess_1",
                    "cmd_test",
                    None,
                    std::time::Duration::from_secs(2),
                    true,
                )
                .await
                .unwrap()
        });
        tokio::task::yield_now().await;
        broker.append_delta("cmd_test", CommandSessionStream::Stdout, b"live\n");
        let event = events.recv().await.unwrap();
        assert_eq!(event.event_type, "command_session_delta");
        assert_eq!(event.data["delta"], "live\n");
        assert_eq!(event.user_id, "user_a");
        assert_eq!(event.session_id, "sess_1");
        tokio::task::yield_now().await;
        assert!(!poll.is_finished(), "output must not complete an exit wait");
        broker.finish_session("cmd_test", Some(0), false, None);
        let result = tokio::time::timeout(std::time::Duration::from_millis(500), poll)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result["status"], "exited");
        assert_eq!(result["stdout"], "live\n");
        assert_eq!(result["dropped"], false);
    }

    #[tokio::test]
    async fn incremental_poll_bounds_output_and_reports_an_unread_gap() {
        let mut broker = CommandSessionBroker::new();
        broker.ring_buffer_bytes = 8;
        broker.start_session(build_start_spec());
        broker.append_delta("cmd_test", CommandSessionStream::Stdout, b"discarded");
        broker.append_delta("cmd_test", CommandSessionStream::Stderr, b"kept");
        let result = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                Some(0),
                std::time::Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        assert_eq!(result["stdout"], "");
        assert_eq!(result["stderr"], "kept");
        assert_eq!(result["dropped"], true);
        broker.finish_session("cmd_test", Some(0), false, None);
        let result = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                true,
            )
            .await
            .unwrap();
        assert_eq!(result["dropped"], false);
        assert_eq!(result["stdout"], "");
    }

    #[tokio::test]
    async fn websocket_command_stream_hydrates_and_isolates_thread_output() {
        use crate::api::ws_helpers::{watch_command_sessions, WsSender};
        let broker = Arc::new(CommandSessionBroker::new());
        broker.start_session(build_start_spec());
        let mut foreign = build_start_spec();
        foreign.command_session_id = Some("cmd_foreign".into());
        foreign.session_id = "sess_other".into();
        broker.start_session(foreign);
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let cancel = CancellationToken::new();
        let stream = tokio::spawn(watch_command_sessions(
            broker.clone(),
            "user_a".into(),
            "sess_1".into(),
            Some("request".into()),
            WsSender::new(tx),
            cancel.clone(),
        ));
        let initial = rx.recv().await.unwrap().into_text().unwrap();
        let initial: serde_json::Value = serde_json::from_str(&initial).unwrap();
        assert_eq!(initial["payload"]["event"], "command_session_summary");
        broker.append_delta("cmd_foreign", CommandSessionStream::Stdout, b"foreign");
        broker.append_delta("cmd_test", CommandSessionStream::Stdout, b"live");
        let live = rx.recv().await.unwrap().into_text().unwrap();
        let live: serde_json::Value = serde_json::from_str(&live).unwrap();
        assert_eq!(live["payload"]["event"], "command_session_delta");
        assert_eq!(live["payload"]["data"]["delta"], "live");
        broker.finish_session("cmd_test", Some(0), false, None);
        let exit = rx.recv().await.unwrap().into_text().unwrap();
        let exit: serde_json::Value = serde_json::from_str(&exit).unwrap();
        assert_eq!(exit["payload"]["event"], "command_session_exit");
        cancel.cancel();
        tokio::time::timeout(std::time::Duration::from_millis(500), stream)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn terminate_task_falls_back_to_the_session_cancel_token() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        // 既没有进程句柄也没有取消令牌时直接收尾，避免面板残留僵尸任务。
        assert!(broker.terminate_task("user_a", "sess_1", "cmd_test"));
        assert!(broker.running_tasks("user_a", "sess_1").is_empty());

        let mut spec = build_start_spec();
        spec.command_session_id = Some("cmd_token".to_string());
        broker.start_session(spec);
        let token = broker.register_cancel("cmd_token");
        assert!(broker.terminate_task("user_a", "sess_1", "cmd_token"));
        assert!(token.is_cancelled());
        // 跨线程作用域依旧拒绝。
        assert!(!broker.terminate_task("user_b", "sess_1", "cmd_token"));
    }

    #[test]
    fn terminate_scope_only_cancels_matching_live_processes() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        broker.start_session(CommandSessionStartSpec {
            command_session_id: Some("cmd_other".to_string()),
            tool_call_id: None,
            user_id: "user_b".to_string(),
            session_id: "sess_1".to_string(),
            workspace_id: "ws_1".to_string(),
            command_index: 0,
            command: "sleep".to_string(),
            cwd: "/tmp".to_string(),
            shell: None,
            launch_mode: CommandSessionLaunchMode::Direct,
            tty: false,
            interactive: false,
        });
        let own = Arc::new(CommandProcessHandle::new(None));
        let other = Arc::new(CommandProcessHandle::new(None));
        assert!(broker.register_process("cmd_test", Arc::clone(&own)));
        assert!(broker.register_process("cmd_other", Arc::clone(&other)));

        assert_eq!(broker.terminate_scope("user_a", "sess_1"), 1);
        assert!(own.cancellation_token().is_cancelled());
        assert!(!other.cancellation_token().is_cancelled());
    }

    #[test]
    fn terminate_scope_cancels_foreground_session_tokens() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        let token = broker.register_cancel("cmd_test");
        assert_eq!(broker.terminate_scope("user_a", "sess_1"), 1);
        assert!(token.is_cancelled());
    }

    #[test]
    fn terminate_task_finishes_session_without_execution_handle() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        // 工具 future 被丢弃后既没有进程句柄也没有取消令牌，终止应直接收尾。
        assert!(broker.terminate_task("user_a", "sess_1", "cmd_test"));
        let snapshot = broker.snapshot("cmd_test").expect("session snapshot");
        assert_eq!(snapshot.status, CommandSessionStatus::Exited);
        assert!(broker.running_tasks("user_a", "sess_1").is_empty());
    }

    #[test]
    fn finish_session_if_running_is_idempotent() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        assert!(broker
            .finish_session_if_running("cmd_test", "ended without completion")
            .is_some());
        assert!(broker
            .finish_session_if_running("cmd_test", "ended without completion")
            .is_none());
    }

    #[test]
    fn broker_enforces_active_process_limit_under_parallel_registration() {
        let broker = Arc::new(CommandSessionBroker::new());
        let accepted = std::thread::scope(|scope| {
            let handles = (0..MAX_ACTIVE_COMMAND_PROCESSES * 2)
                .map(|index| {
                    let broker = Arc::clone(&broker);
                    scope.spawn(move || {
                        let id = format!("cmd_parallel_{index}");
                        let mut spec = build_start_spec();
                        spec.command_session_id = Some(id.clone());
                        broker.start_session(spec);
                        broker.register_process(&id, Arc::new(CommandProcessHandle::new(None)))
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .filter(|accepted| *accepted)
                .count()
        });
        assert_eq!(accepted, MAX_ACTIVE_COMMAND_PROCESSES);
    }

    #[tokio::test]
    async fn stdin_write_reports_closed_and_exited_without_failing() {
        let broker = CommandSessionBroker::new();
        broker.start_session(build_start_spec());
        // 非交互会话（没有 stdin 句柄）：写入要能识别为「没有可写 stdin」。
        assert!(broker.register_process("cmd_test", Arc::new(CommandProcessHandle::new(None))));
        assert_eq!(
            broker
                .write_stdin("user_a", "sess_1", "cmd_test", b"yes\n")
                .await
                .unwrap(),
            CommandStdinWrite::StdinClosed
        );
        // 会话结束后再写：不是错误，调用方仍可读最终输出。
        broker.finish_session("cmd_test", Some(0), false, None);
        assert_eq!(
            broker
                .write_stdin("user_a", "sess_1", "cmd_test", b"yes\n")
                .await
                .unwrap(),
            CommandStdinWrite::SessionExited
        );
        let final_data = broker
            .poll(
                "user_a",
                "sess_1",
                "cmd_test",
                None,
                std::time::Duration::ZERO,
                false,
            )
            .await
            .unwrap();
        assert_eq!(final_data["status"], "exited");
        assert_eq!(final_data["exit_code"], 0);
        // 跨作用域仍然拒绝。
        assert!(broker
            .write_stdin("user_b", "sess_1", "cmd_test", b"x")
            .await
            .is_err());
    }
}
