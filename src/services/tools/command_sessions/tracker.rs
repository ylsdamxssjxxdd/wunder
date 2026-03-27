use super::broker::CommandSessionBroker;
use super::types::{
    CommandSessionLaunchMode, CommandSessionStartSpec, CommandSessionStatus, CommandSessionStream,
};
use crate::services::tools::{ToolContext, ToolEventEmitter};
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct CommandSessionTracker {
    broker: Option<Arc<CommandSessionBroker>>,
    emitter: Option<ToolEventEmitter>,
    command_session_id: String,
    command_index: usize,
    fallback_seq: Arc<AtomicU64>,
}

impl CommandSessionTracker {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        context: &ToolContext<'_>,
        command: &str,
        cwd: &str,
        command_index: usize,
        shell: Option<String>,
        launch_mode: CommandSessionLaunchMode,
        tty: bool,
        interactive: bool,
    ) -> Option<Self> {
        let broker = context.command_sessions.as_ref().map(Arc::clone);
        let emitter = context.event_emitter.clone();
        if broker.is_none() && emitter.is_none() {
            return None;
        }

        let tool_call_id = emitter
            .as_ref()
            .and_then(|item| item.default_string_field("tool_call_id"));
        let start_spec = CommandSessionStartSpec {
            command_session_id: None,
            tool_call_id,
            user_id: context.user_id.to_string(),
            session_id: context.session_id.to_string(),
            workspace_id: context.workspace_id.to_string(),
            command_index,
            command: command.to_string(),
            cwd: cwd.to_string(),
            shell,
            launch_mode,
            tty,
            interactive,
        };
        let snapshot = if let Some(broker) = broker.as_ref() {
            broker.start_session(start_spec)
        } else {
            let command_session_id = CommandSessionBroker::generate_session_id();
            let mut spec = start_spec;
            spec.command_session_id = Some(command_session_id.clone());
            super::types::CommandSessionSnapshot {
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
                started_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                ended_at: None,
                exit_code: None,
                timed_out: false,
                error: None,
                stdout_bytes: 0,
                stderr_bytes: 0,
                pty_bytes: 0,
                stdout_dropped_bytes: 0,
                stderr_dropped_bytes: 0,
                pty_dropped_bytes: 0,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                pty_tail: String::new(),
            }
        };
        if let Some(emitter) = emitter.as_ref() {
            emitter.emit("command_session_start", snapshot.start_event_payload());
        }
        Some(Self {
            broker,
            emitter,
            command_session_id: snapshot.command_session_id,
            command_index,
            fallback_seq: Arc::new(AtomicU64::new(snapshot.seq)),
        })
    }

    pub(crate) fn command_session_id(&self) -> &str {
        &self.command_session_id
    }

    pub(crate) fn decorate_legacy_payload(&self, map: &mut Map<String, Value>) {
        map.insert(
            "command_session_id".to_string(),
            Value::String(self.command_session_id.clone()),
        );
        map.insert("command_index".to_string(), json!(self.command_index));
    }

    pub(crate) fn emit_delta(&self, stream: CommandSessionStream, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let _seq = if let Some(broker) = self.broker.as_ref() {
            broker
                .append_delta(&self.command_session_id, stream, chunk)
                .unwrap_or_else(|| self.fallback_seq.fetch_add(1, Ordering::Relaxed) + 1)
        } else {
            self.fallback_seq.fetch_add(1, Ordering::Relaxed) + 1
        };
    }

    pub(crate) fn emit_failed_to_start(&self, error: impl Into<String>) {
        let error = error.into();
        let snapshot = if let Some(broker) = self.broker.as_ref() {
            broker
                .mark_failed_to_start(&self.command_session_id, error.clone())
                .or_else(|| broker.snapshot(&self.command_session_id))
        } else {
            None
        };
        if let (Some(emitter), Some(snapshot)) = (self.emitter.as_ref(), snapshot.as_ref()) {
            emitter.emit("command_session_status", snapshot.status_event_payload());
            emitter.emit("command_session_summary", snapshot.summary_event_payload());
        } else if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(
                "command_session_status",
                json!({
                    "command_session_id": self.command_session_id,
                    "command_index": self.command_index,
                    "status": "failed_to_start",
                    "error": error,
                }),
            );
        }
    }

    pub(crate) fn emit_exit(&self, exit_code: Option<i32>, timed_out: bool, error: Option<String>) {
        let snapshot = if let Some(broker) = self.broker.as_ref() {
            broker.finish_session(
                &self.command_session_id,
                exit_code,
                timed_out,
                error.clone(),
            )
        } else {
            None
        };
        if let (Some(emitter), Some(snapshot)) = (self.emitter.as_ref(), snapshot.as_ref()) {
            emitter.emit("command_session_exit", snapshot.exit_event_payload());
            emitter.emit("command_session_summary", snapshot.summary_event_payload());
        } else if let Some(emitter) = self.emitter.as_ref() {
            emitter.emit(
                "command_session_exit",
                json!({
                    "command_session_id": self.command_session_id,
                    "command_index": self.command_index,
                    "status": "exited",
                    "exit_code": exit_code,
                    "timed_out": timed_out,
                    "error": error,
                }),
            );
        }
    }
}
