//! Desktop terminal façade.
//!
//! Thin synchronous wrappers around the runtime's `DesktopTerminalService`.
//! The runtime service spawns a persistent interactive shell (cmd.exe on
//! Windows, bash on Unix), on a winpty console where the host ships one and on
//! piped stdin/stdout/stderr otherwise, and streams decoded output through a
//! seq-addressed bounded buffer; this file only converts the UI's typed requests
//! into blocking calls on the shared tokio runtime and back into plain
//! UI-friendly types. No scheduling or sandbox rules are duplicated here.

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::runtime::Runtime;
use wunder_server::{
    storage::StorageBackend, DesktopTerminalBackend, DesktopTerminalFrame, DesktopTerminalService,
    DesktopTerminalSnapshot, DesktopTerminalStartSpec, DesktopTerminalStatus,
};

/// Typed start request from the native UI.
#[derive(Debug, Clone)]
pub struct NativeTerminalSpec {
    /// Optional caller-chosen id; empty generates `term_<uuid>`.
    pub terminal_id: String,
    pub session_id: String,
    pub command: String,
    pub cwd: String,
    /// Empty chooses the platform default: cmd.exe (Windows) / bash (Unix).
    pub shell: String,
}

/// Snapshot alias so the native UI never depends on runtime path details.
pub type NativeTerminalSnapshot = DesktopTerminalSnapshot;

/// UI-friendly projection of a poll result plus the id it belonged to.
#[derive(Debug, Clone)]
pub struct NativeTerminalFrame {
    pub terminal_id: String,
    pub seq: u64,
    pub truncated: bool,
    pub text: String,
    pub status: String,
    /// "console" when the shell owns a real terminal, "pipes" otherwise. The
    /// panel shows it, because what the user can do differs between them.
    pub backend: String,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

impl NativeTerminalFrame {
    fn from_frame(terminal_id: &str, frame: DesktopTerminalFrame) -> Self {
        let status = match frame.status {
            DesktopTerminalStatus::Starting => "starting",
            DesktopTerminalStatus::Running => "running",
            DesktopTerminalStatus::FailedToStart => "failed",
            DesktopTerminalStatus::Exited => "exited",
        }
        .to_string();
        Self {
            terminal_id: terminal_id.to_string(),
            seq: frame.seq,
            truncated: frame.truncated,
            text: frame.text,
            status,
            backend: match frame.backend {
                DesktopTerminalBackend::Console => "console",
                DesktopTerminalBackend::Pipes => "pipes",
            }
            .to_string(),
            exit_code: frame.exit_code,
            error: frame.error,
        }
    }
}

/// Synchronous entry point used by the native frontend. Owns the runtime
/// terminal service instance (scoped to the desktop runtime lifetime).
pub struct NativeTerminal {
    runtime: Arc<Runtime>,
    service: Arc<DesktopTerminalService>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> Arc<Runtime> {
        Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build test runtime"),
        )
    }

    fn temp_storage(name: &str) -> Arc<dyn StorageBackend> {
        let dir = std::env::temp_dir().join(format!("wunder_terminal_{name}"));
        // Hermetic: never inherit chunks from an earlier run of the suite.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp storage dir");
        Arc::new(wunder_server::storage::SqliteStorage::new(
            dir.join("transcript.db").to_string_lossy().to_string(),
        ))
    }

    #[test]
    fn start_send_poll_cancel_roundtrip() {
        let user_id = "terminal_test_user";
        let session_id = "terminal_test_session";
        let terminal = NativeTerminal::new(runtime(), temp_storage("roundtrip"), None);
        let spec = NativeTerminalSpec {
            terminal_id: String::new(),
            session_id: session_id.to_string(),
            command: String::new(),
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            shell: String::new(),
        };
        let snapshot = terminal
            .start(user_id, &spec)
            .expect("terminal should start");
        let terminal_id = snapshot.terminal_id;

        // Poll until Running or FailedToStart appears.
        let mut started = false;
        for _ in 0..20 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, 0)
                .expect("poll should not fail");
            if frame.status == "running" {
                started = true;
                break;
            }
            if frame.status == "failed" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(started, "terminal should reach running status");

        let prompt = if cfg!(windows) {
            "echo wunder_ok\r\n"
        } else {
            "echo wunder_ok\n"
        };
        terminal
            .send(user_id, session_id, &terminal_id, prompt.as_bytes())
            .expect("send should succeed");

        // Give the reader loop time to decode the echoed line.
        let mut got_output = false;
        for _ in 0..40 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, 0)
                .expect("poll should not fail");
            if frame.text.contains("wunder_ok") {
                got_output = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(got_output, "terminal output should contain the echoed line");

        let frame = terminal
            .poll(user_id, session_id, &terminal_id, 0)
            .expect("poll after output should not fail");
        assert!(frame.seq > 0, "seq must advance");

        // Cancel kills the process; the record then reports exit.
        terminal
            .cancel(user_id, session_id, &terminal_id)
            .expect("cancel should succeed");
        let mut exited = false;
        for _ in 0..40 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, 0)
                .expect("poll should not fail");
            if frame.status == "exited" || frame.status == "failed" {
                exited = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(exited, "terminal should exit after cancel");

        terminal
            .close(user_id, session_id, &terminal_id)
            .expect("close should succeed");
        assert!(
            terminal.list(user_id, session_id).is_empty(),
            "closed terminal should be gone from the list"
        );
    }

    fn spec(session_id: &str) -> NativeTerminalSpec {
        NativeTerminalSpec {
            terminal_id: String::new(),
            session_id: session_id.to_string(),
            command: String::new(),
            cwd: std::env::temp_dir().to_string_lossy().to_string(),
            shell: String::new(),
        }
    }

    #[test]
    fn transcript_survives_a_restart() {
        let user_id = "terminal_test_user";
        let session_id = "transcript_test_session";
        let marker = "wunder_resume_marker";
        let storage = temp_storage("resume");
        let runtime = runtime();
        {
            let terminal = NativeTerminal::new(Arc::clone(&runtime), Arc::clone(&storage), None);
            let snapshot = terminal.start(user_id, &spec(session_id)).expect("start");
            let terminal_id = snapshot.terminal_id;
            // stdin only exists once the spawn task has wired the pipes.
            for _ in 0..20 {
                let frame = terminal
                    .poll(user_id, session_id, &terminal_id, 0)
                    .expect("poll should not fail");
                if frame.status == "running" {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let prompt = if cfg!(windows) {
                format!("echo {marker}\r\n")
            } else {
                format!("echo {marker}\n")
            };
            terminal
                .send(user_id, session_id, &terminal_id, prompt.as_bytes())
                .expect("send should succeed");
            for _ in 0..40 {
                let frame = terminal
                    .poll(user_id, session_id, &terminal_id, 0)
                    .expect("poll should not fail");
                if frame.text.contains(marker) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            // close() force-spills the buffered tail of the run.
            terminal
                .close(user_id, session_id, &terminal_id)
                .expect("close should succeed");
        }
        // A fresh facade over the same storage is exactly the restart path.
        let restored = NativeTerminal::new(runtime, storage, None).transcript(user_id, session_id);
        assert!(
            restored.contains(marker),
            "transcript should survive restart, got {restored:?}"
        );
    }

    /// End-to-end check of the console backend. It needs a winpty built for the
    /// same word size as the test process, so it stays opt-in: point
    /// `WUNDER_WINPTY_DLL` at a `winpty.dll` whose `winpty-agent.exe` sits
    /// beside it, then run
    /// `cargo test -p wunder-desktop --features terminal-pty --lib -- --ignored`
    #[cfg(all(windows, feature = "terminal-pty"))]
    #[test]
    #[ignore]
    fn console_echoes_resizes_and_survives_interrupt() {
        let dll = std::env::var_os("WUNDER_WINPTY_DLL")
            .map(std::path::PathBuf::from)
            .expect("set WUNDER_WINPTY_DLL to a winpty.dll of this word size");
        let user_id = "terminal_test_user";
        let session_id = "terminal_console_session";
        let terminal = NativeTerminal::new(runtime(), temp_storage("console"), Some(dll));
        let terminal_id = terminal
            .start(user_id, &spec(session_id))
            .expect("console start")
            .terminal_id;

        let mut backend = String::new();
        let mut seq = 0u64;
        let mut seen = String::new();
        for _ in 0..60 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, seq)
                .expect("poll should not fail");
            seq = frame.seq;
            seen.push_str(&frame.text);
            backend = frame.backend.clone();
            if frame.status == "running" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(backend, "console", "the shell must be on a winpty console");

        // Wait until the shell stops drawing: a line typed while its line editor
        // is still starting is taken in without ever being echoed.
        let mut quiet = 0;
        for _ in 0..80 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, seq)
                .expect("poll should not fail");
            seq = frame.seq;
            seen.push_str(&frame.text);
            quiet = if frame.text.is_empty() { quiet + 1 } else { 0 };
            if quiet >= 6 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Typed input comes back as an echo *and* as the command's own output,
        // which is what a real terminal device does and pipes never did.
        let marker = "wunder_console_marker";
        terminal
            .send(
                user_id,
                session_id,
                &terminal_id,
                format!("echo {marker}\r\n").as_bytes(),
            )
            .expect("console input should be accepted");
        let mut echoes = 0;
        for _ in 0..60 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, seq)
                .expect("poll should not fail");
            seq = frame.seq;
            seen.push_str(&frame.text);
            echoes = seen.matches(marker).count();
            if echoes >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(echoes >= 2, "a console echoes what was typed, got {seen:?}");

        terminal
            .resize(user_id, session_id, &terminal_id, 40, 12)
            .expect("the console should accept a new size");

        // On a console this is Ctrl-C: the shell survives and answers again.
        terminal
            .cancel(user_id, session_id, &terminal_id)
            .expect("interrupt should be deliverable");
        let marker = "wunder_after_interrupt";
        terminal
            .send(
                user_id,
                session_id,
                &terminal_id,
                format!("echo {marker}\r\n").as_bytes(),
            )
            .expect("the shell should still take input after Ctrl-C");
        let mut answered = false;
        for _ in 0..60 {
            let frame = terminal
                .poll(user_id, session_id, &terminal_id, seq)
                .expect("poll should not fail");
            seq = frame.seq;
            seen.push_str(&frame.text);
            if seen.contains(marker) && frame.status == "running" {
                answered = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(answered, "the shell must stay up after an interrupt");

        terminal
            .close(user_id, session_id, &terminal_id)
            .expect("close should stop the console");
        assert!(terminal.list(user_id, session_id).is_empty());
    }
}

impl NativeTerminal {
    /// `storage` backs the durable transcript: shell output is spilled in
    /// bounded chunks so a restart can restore what the panel showed before.
    /// `winpty` is the console library to run shells under, when the host has
    /// one; `None` keeps every shell on pipes.
    pub fn new(
        runtime: Arc<Runtime>,
        storage: Arc<dyn StorageBackend>,
        winpty: Option<PathBuf>,
    ) -> Self {
        Self {
            runtime,
            service: Arc::new(
                DesktopTerminalService::new()
                    .with_transcript_storage(storage)
                    .with_winpty(winpty),
            ),
        }
    }

    /// Raw transcript of a session scope, oldest output first. Empty when
    /// nothing was persisted yet.
    pub fn transcript(&self, user_id: &str, session_id: &str) -> String {
        self.service.transcript(user_id, session_id).0
    }

    fn start_spec(user_id: &str, spec: &NativeTerminalSpec) -> DesktopTerminalStartSpec {
        DesktopTerminalStartSpec {
            terminal_id: (!spec.terminal_id.trim().is_empty())
                .then(|| spec.terminal_id.trim().to_string()),
            user_id: user_id.to_string(),
            session_id: spec.session_id.trim().to_string(),
            workspace_id: String::new(),
            command: (!spec.command.trim().is_empty()).then(|| spec.command.trim().to_string()),
            cwd: spec.cwd.clone(),
            shell: (!spec.shell.trim().is_empty()).then(|| spec.shell.trim().to_string()),
        }
    }

    /// Start (or reuse) a terminal session for `user_id`/`session_id`.
    /// `DesktopTerminalService::start` requires a live tokio context to spawn
    /// the process pump, so drive it inside the shared runtime.
    pub fn start(
        &self,
        user_id: &str,
        spec: &NativeTerminalSpec,
    ) -> Result<DesktopTerminalSnapshot> {
        let service = Arc::clone(&self.service);
        let spec = Self::start_spec(user_id, spec);
        self.runtime
            .block_on(async move { service.start(spec) })
            .map_err(|message| anyhow!(message))
    }

    /// Fetch the incremental output frame since `last_seq`.
    pub fn poll(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        last_seq: u64,
    ) -> Result<NativeTerminalFrame> {
        let frame = self
            .service
            .poll(user_id, session_id, terminal_id, last_seq)
            .map_err(|message| anyhow!(message))?;
        Ok(NativeTerminalFrame::from_frame(terminal_id, frame))
    }

    /// Send raw bytes to the terminal stdin.
    pub fn send(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        input: &[u8],
    ) -> Result<()> {
        self.runtime
            .block_on(
                self.service
                    .write_stdin(user_id, session_id, terminal_id, input),
            )
            .map_err(|message| anyhow!(message))
    }

    /// Interrupt the shell: Ctrl-C where it owns a console, an OS-level kill
    /// where it does not, because a piped shell can be given no signal at all.
    pub fn cancel(&self, user_id: &str, session_id: &str, terminal_id: &str) -> Result<()> {
        self.service
            .interrupt(user_id, session_id, terminal_id)
            .map_err(|message| anyhow!(message))
    }

    /// Report the panel's cell grid to the shell, so prompts wrap and full-screen
    /// programs repaint at the width the user actually has.
    pub fn resize(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<()> {
        self.service
            .resize(user_id, session_id, terminal_id, cols, rows)
            .map_err(|message| anyhow!(message))
    }

    /// Drop the terminal record (also kills the process via cancellation).
    pub fn close(&self, user_id: &str, session_id: &str, terminal_id: &str) -> Result<()> {
        self.service
            .close(user_id, session_id, terminal_id)
            .map_err(|message| anyhow!(message))
    }

    /// List live terminal snapshots for the given session.
    pub fn list(&self, user_id: &str, session_id: &str) -> Vec<DesktopTerminalSnapshot> {
        self.service.list(user_id, session_id)
    }
}
