//! Desktop terminal service.
//!
//! Long-lived interactive shell sessions for the native (Slint) desktop UI.
//! Unlike the model-facing `CommandSessionBroker` (short-lived detached
//! command runs with head/tail previews), this service keeps a persistent
//! shell process (cmd.exe on Windows, bash on Unix) with an open stdin,
//! streams both stdout and stderr into an incremental, seq-addressed output
//! buffer and exposes launch/poll/write/cancel operations. Output bytes are
//! decoded incrementally so multi-byte characters split across chunk
//! boundaries survive intact (UTF-8 natively, GBK fallback on Windows).
#[cfg(all(windows, feature = "terminal-pty"))]
use super::{terminal_conpty, terminal_pty};
use crate::storage::StorageBackend;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
#[cfg(unix)]
use portable_pty::{ChildKiller, MasterPty, PtySize};
use serde_json::Value;
use std::collections::VecDeque;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const TERMINAL_OUTPUT_BUDGET_BYTES: usize = 1024 * 1024;
const MAX_ACTIVE_TERMINALS: usize = 8;
const PIPE_CHUNK_BYTES: usize = 8192;
/// Durable transcript spill thresholds: a chunk lands when it is big enough to
/// amortize the write, when the stream went quiet, or when the run ends.
const TRANSCRIPT_FLUSH_BYTES: usize = 4096;
const TRANSCRIPT_FLUSH_INTERVAL: Duration = Duration::from_millis(500);
/// Newest chunks kept per session scope, bounding the durable transcript to
/// roughly 8 MiB regardless of how long a terminal lives.
const TRANSCRIPT_KEEP_CHUNKS: i64 = 2048;
/// Chunks read back by one restore, keeping startup work bounded.
const TRANSCRIPT_RESTORE_CHUNKS: i64 = 256;
/// Cell size a shell starts with, before the panel reports its own geometry.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopTerminalStatus {
    Starting,
    Running,
    FailedToStart,
    Exited,
}

/// Find a usable winpty among the directories that may hold it. The library and
/// its agent have to sit together, because the agent is located from the
/// library's own directory; a directory or a direct path to the library both
/// work. Any platform can ask, since this only looks at files.
pub fn find_winpty_library(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find_map(|candidate| {
        let dll = if candidate.is_dir() {
            candidate.join("winpty.dll")
        } else {
            candidate.clone()
        };
        let agent = dll.with_file_name("winpty-agent.exe");
        (dll.is_file() && agent.is_file() && pe_machine(&dll) == Some(HOST_WINPTY_MACHINE))
            .then_some(dll)
    })
}

/// `IMAGE_FILE_MACHINE_I386`.
const MACHINE_I386: u16 = 0x014c;
/// `IMAGE_FILE_MACHINE_AMD64`.
const MACHINE_AMD64: u16 = 0x8664;
/// The machine a winpty must carry to be loadable into this process. A 32-bit
/// library cannot be mapped by a 64-bit process and the reverse is just as true,
/// so asking the header here keeps a foreign winpty from being chosen over a
/// usable one and from failing the same load on every terminal start.
const HOST_WINPTY_MACHINE: u16 = if cfg!(target_pointer_width = "64") {
    MACHINE_AMD64
} else {
    MACHINE_I386
};

/// The machine word of a PE file, read without loading it. `None` for anything
/// that is not a recognizable PE, which is also the answer for a winpty that
/// cannot be used.
fn pe_machine(path: &Path) -> Option<u16> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = [0u8; 512];
    let mut filled = 0;
    while filled < head.len() {
        let read = file.read(&mut head[filled..]).ok()?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    // `e_lfanew` points at the `PE\0\0` signature, followed by the machine word.
    let offset = u32::from_le_bytes(head[0x3c..0x40].try_into().ok()?) as usize;
    let header = head.get(offset..offset + 6)?;
    if header[..4] != *b"PE\0\0" {
        return None;
    }
    Some(u16::from_le_bytes([header[4], header[5]]))
}

/// Which terminal device the shell is attached to. A console changes what the
/// shell will do for the client: it echoes what was typed, repaints in place,
/// and takes Ctrl-C, and programs that check whether they own a terminal stop
/// downgrading their output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopTerminalBackend {
    Pipes,
    Console,
    Pty,
}

/// The console object, where one exists. Elsewhere this is a placeholder that
/// can never be filled, so the record layout stays the same on every platform.
#[cfg(all(windows, feature = "terminal-pty"))]
enum Console {
    Native(Arc<terminal_conpty::NativeConsole>),
    Legacy(Arc<terminal_pty::PtyInner>),
}
#[cfg(all(windows, feature = "terminal-pty"))]
impl Console {
    fn set_size(&self, cols: u16, rows: u16) -> Result<(), String> {
        match self {
            Self::Native(c) => c.set_size(cols, rows),
            Self::Legacy(c) => c.set_size(cols, rows),
        }
    }
    fn wait(&self) -> Option<i32> {
        match self {
            Self::Native(c) => c.wait(),
            Self::Legacy(c) => c.wait(),
        }
    }
    fn shutdown(&self) {
        match self {
            Self::Native(c) => c.shutdown(),
            Self::Legacy(c) => c.shutdown(),
        }
    }
}
#[cfg(not(all(windows, feature = "terminal-pty")))]
type Console = ();

/// Where keystrokes go.
#[derive(Clone)]
enum InputSink {
    /// The shell's stdin pipe: nothing is echoed and no signal can be delivered.
    Child(Arc<tokio::sync::Mutex<ChildStdin>>),
    /// The console's input pipe: the shell echoes, and a Ctrl-C byte means
    /// Ctrl-C. Writes block on a small pipe buffer, so they run off the task.
    Console(Arc<File>),
    /// Unix pseudo-terminal input, which delivers control bytes and signals.
    #[cfg(unix)]
    Pty(Arc<UnixPtyHandle>),
}

enum BlockingTerminalIo {
    Console(Arc<File>),
    #[cfg(unix)]
    Pty(Arc<UnixPtyHandle>),
}

#[cfg(unix)]
struct UnixPtyHandle {
    master: Mutex<Box<dyn MasterPty + Send>>,
    input: Mutex<Box<dyn std::io::Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

#[cfg(unix)]
struct UnixPtySession {
    handle: Arc<UnixPtyHandle>,
    reader: Box<dyn std::io::Read + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DesktopTerminalSnapshot {
    pub terminal_id: String,
    pub user_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub command: String,
    pub cwd: String,
    pub shell: String,
    pub status: DesktopTerminalStatus,
    pub backend: DesktopTerminalBackend,
    pub seq: u64,
    pub dropped_bytes: u64,
    pub truncated: bool,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DesktopTerminalFrame {
    pub terminal_id: String,
    pub seq: u64,
    /// True when the caller's last_seq was older than the retained buffer
    /// head; the frontend must clear and rerender from `text`.
    pub truncated: bool,
    /// Incremental decoded output since the caller's last_seq.
    pub text: String,
    pub status: DesktopTerminalStatus,
    pub backend: DesktopTerminalBackend,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DesktopTerminalStartSpec {
    pub terminal_id: Option<String>,
    pub user_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub command: Option<String>,
    pub cwd: String,
    pub shell: Option<String>,
}

/// Streaming decoder that only emits complete characters. Bytes that can
/// continue a multi-byte sequence are retained until the next chunk.
struct IncrementalDecoder {
    pending: Vec<u8>,
    gbk_fallback: bool,
}

impl IncrementalDecoder {
    fn new(gbk_fallback: bool) -> Self {
        Self {
            pending: Vec::with_capacity(32),
            gbk_fallback,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> String {
        self.pending.extend_from_slice(chunk);
        let mut output = String::new();
        loop {
            if self.pending.is_empty() {
                return output;
            }
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    output.push_str(text);
                    self.pending.clear();
                    return output;
                }
                Err(error) => {
                    let valid_up_to = error.valid_up_to();
                    if valid_up_to > 0 {
                        match std::str::from_utf8(&self.pending[..valid_up_to]) {
                            Ok(text) => output.push_str(text),
                            Err(_) => output
                                .push_str(&String::from_utf8_lossy(&self.pending[..valid_up_to])),
                        }
                        self.pending.drain(..valid_up_to);
                        continue;
                    }
                    // At least one byte at the head is not valid UTF-8.
                    if self.gbk_fallback {
                        let before = self.pending.len();
                        let decoded = self.decode_gbk_prefix();
                        output.push_str(&decoded);
                        // `decode_gbk_prefix` may retain a lone GBK lead byte
                        // waiting for its trailing half; no progress means we
                        // must stop and wait for the next chunk.
                        if decoded.is_empty() && self.pending.len() == before {
                            return output;
                        }
                    } else {
                        // Hold an incomplete UTF-8 multi-byte lead until the
                        // sequence completes across chunk boundaries.
                        let lead = self.pending[0];
                        let expected = utf8_sequence_len(lead);
                        if expected > 1 && self.pending.len() < expected {
                            return output;
                        }
                        output.push_str(&String::from_utf8_lossy(&self.pending[..1]));
                        self.pending.drain(..1);
                    }
                }
            }
        }
    }

    /// Decode as much of `pending` as forms complete GBK characters,
    /// retaining a trailing lead byte that may combine with the next chunk.
    fn decode_gbk_prefix(&mut self) -> String {
        let mut end = self.pending.len();
        if end > 0 {
            // GBK lead bytes are 0x81..=0xFE, but a complete pair's trailing
            // byte can also fall inside that range (e.g. 0xD6 0xD0). Only a
            // trailing run of an odd number of lead-range bytes leaves a lone
            // lead byte that must wait for its pair.
            let trailing_lead = self
                .pending
                .iter()
                .rev()
                .take_while(|byte| (0x81u8..=0xFE).contains(byte))
                .count();
            if trailing_lead % 2 == 1 {
                end -= 1;
            }
        }
        if end == 0 {
            return String::new();
        }
        let slice = self.pending[..end].to_vec();
        let (decoded, _, _) = encoding_rs::GBK.decode(&slice);
        self.pending.drain(..end);
        decoded.into_owned()
    }
}

/// Byte length of the UTF-8 sequence headed by `lead`; 1 when it cannot
/// start a multi-byte sequence (ASCII or an invalid lead byte).
fn utf8_sequence_len(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

struct OutputBuffer {
    /// (seq, decoded text) chunks, oldest first.
    chunks: VecDeque<(u64, String)>,
    /// Absolute seq of the last appended chunk.
    seq: u64,
    /// Seq of the oldest retained chunk (== first seq ever buffered until
    /// head drops). Used to detect caller truncation.
    min_seq: u64,
    retained_bytes: usize,
    dropped_bytes: u64,
    budget_bytes: usize,
}

impl OutputBuffer {
    fn new(budget_bytes: usize) -> Self {
        Self {
            chunks: VecDeque::new(),
            seq: 0,
            min_seq: 0,
            retained_bytes: 0,
            dropped_bytes: 0,
            budget_bytes,
        }
    }

    fn append(&mut self, text: &str) -> u64 {
        if text.is_empty() {
            return self.seq;
        }
        if text.len() > self.budget_bytes {
            self.chunks.clear();
            self.min_seq = self.seq + 1;
            self.retained_bytes = 0;
            self.dropped_bytes = self.dropped_bytes.saturating_add(text.len() as u64);
            return self.seq;
        }
        self.seq += 1;
        self.chunks.push_back((self.seq, text.to_string()));
        self.retained_bytes += text.len();
        self.drop_head_over_budget();
        self.seq
    }

    fn drop_head_over_budget(&mut self) {
        while self.retained_bytes > self.budget_bytes && self.chunks.len() > 1 {
            let (seq, text) = self.chunks.pop_front().expect("non-empty chunks");
            self.min_seq = seq;
            self.retained_bytes = self.retained_bytes.saturating_sub(text.len());
            self.dropped_bytes = self.dropped_bytes.saturating_add(text.len() as u64);
        }
    }

    /// Collect every chunk whose seq is strictly greater than `last_seq`.
    /// Returns (text, truncated) where truncated is true when the caller
    /// missed dropped head data and must redraw.
    fn collect_after(&self, last_seq: u64) -> (String, bool) {
        let truncated = !self.chunks.is_empty() && last_seq < self.min_seq;
        let mut text = String::new();
        for (seq, chunk) in self.chunks.iter() {
            if seq > &last_seq {
                text.push_str(chunk);
            }
        }
        (text, truncated)
    }
}

/// Best-effort durable transcript writer. SQLite never runs under the live
/// record lock. Both the pending chunk and the writer backlog are bounded.
struct TranscriptSink {
    writer: Option<std::sync::mpsc::SyncSender<TranscriptWrite>>,
    failed: Arc<std::sync::atomic::AtomicBool>,
    pending: String,
    last_flush: Instant,
}

enum TranscriptWrite {
    Chunk {
        terminal_id: String,
        seq: u64,
        text: String,
    },
    Flush(std::sync::mpsc::SyncSender<()>),
}

impl TranscriptSink {
    fn open(storage: Option<Arc<dyn StorageBackend>>, user_id: &str, session_id: &str) -> Self {
        let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut sink = Self {
            writer: None,
            failed: failed.clone(),
            pending: String::new(),
            last_flush: Instant::now(),
        };
        let Some(storage) = storage else {
            return sink;
        };
        let user_id = user_id.to_owned();
        let session_id = session_id.to_owned();
        // At most 64 output chunks can wait for storage; live output and input
        // continue even when a local disk is stalled or unavailable.
        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        let worker = std::thread::Builder::new()
            .name("terminal-transcript".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let mut next_chunk = storage
                    .terminal_transcript_next_chunk(&user_id, &session_id)
                    .unwrap_or(0);
                let _ = storage.prune_terminal_transcript(
                    &user_id,
                    &session_id,
                    TRANSCRIPT_KEEP_CHUNKS,
                );
                for event in rx {
                    match event {
                        TranscriptWrite::Chunk {
                            terminal_id,
                            seq,
                            text,
                        } => {
                            if failed.load(Ordering::Acquire) {
                                continue;
                            }
                            if let Err(error) = storage.upsert_terminal_transcript_block(
                                &user_id,
                                &session_id,
                                &terminal_id,
                                next_chunk,
                                seq.min(i64::MAX as u64) as i64,
                                &text,
                            ) {
                                tracing::warn!("terminal transcript write failed: {error}");
                                failed.store(true, Ordering::Release);
                            }
                            next_chunk += 1;
                            if next_chunk % 128 == 0 {
                                let _ = storage.prune_terminal_transcript(
                                    &user_id,
                                    &session_id,
                                    TRANSCRIPT_KEEP_CHUNKS,
                                );
                            }
                        }
                        TranscriptWrite::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            });
        if worker.is_ok() {
            sink.writer = Some(tx);
        }
        sink
    }

    fn queue(&mut self, text: &str) {
        if self.writer.is_some() && !self.failed.load(Ordering::Acquire) {
            self.pending.push_str(text);
        }
    }

    fn spill(&mut self, terminal_id: &str, seq: u64, force: bool) {
        if self.pending.is_empty() {
            return;
        }
        if !force
            && self.pending.len() < TRANSCRIPT_FLUSH_BYTES
            && self.last_flush.elapsed() < TRANSCRIPT_FLUSH_INTERVAL
        {
            return;
        }
        let text = std::mem::take(&mut self.pending);
        self.last_flush = Instant::now();
        if let Some(writer) = &self.writer {
            if writer
                .try_send(TranscriptWrite::Chunk {
                    terminal_id: terminal_id.into(),
                    seq,
                    text,
                })
                .is_err()
            {
                // Persistence is optional. Bound memory instead of blocking
                // the interactive console behind a stalled storage device.
                tracing::warn!(
                    "terminal transcript storage backlog exceeded; recording stopped for this run"
                );
                self.writer = None;
            }
        }
    }
}

struct TerminalRecord {
    snapshot: DesktopTerminalSnapshot,
    output: OutputBuffer,
    input: Mutex<Option<InputSink>>,
    /// Held so the viewport can be resized, and so dropping the record tears the
    /// console down (which takes the shell with it).
    console: Option<Arc<Console>>,
    #[cfg(unix)]
    unix_pty: Option<Arc<UnixPtyHandle>>,
    /// Cell size the shell believes it has.
    size: (u16, u16),
    cancel: CancellationToken,
    transcript: TranscriptSink,
    changes: tokio::sync::watch::Sender<u64>,
}

impl TerminalRecord {
    /// Buffer decoded output in the live ring and queue it for the durable
    /// transcript, spilling when the spill policy says so.
    fn push_output(&mut self, text: &str, force: bool) {
        let seq = self.output.append(text);
        self.transcript.queue(text);
        self.transcript
            .spill(&self.snapshot.terminal_id, seq, force);
        self.changed();
    }

    fn changed(&self) {
        self.changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    fn spill_transcript(&mut self) {
        let seq = self.output.seq;
        self.transcript.spill(&self.snapshot.terminal_id, seq, true);
    }
    fn snapshot(&self) -> DesktopTerminalSnapshot {
        DesktopTerminalSnapshot {
            terminal_id: self.snapshot.terminal_id.clone(),
            user_id: self.snapshot.user_id.clone(),
            session_id: self.snapshot.session_id.clone(),
            workspace_id: self.snapshot.workspace_id.clone(),
            command: self.snapshot.command.clone(),
            cwd: self.snapshot.cwd.clone(),
            shell: self.snapshot.shell.clone(),
            status: self.snapshot.status,
            backend: self.snapshot.backend,
            seq: self.output.seq,
            dropped_bytes: self.output.dropped_bytes,
            truncated: self.output.min_seq > 0,
            started_at: self.snapshot.started_at,
            ended_at: self.snapshot.ended_at,
            exit_code: self.snapshot.exit_code,
            error: self.snapshot.error.clone(),
        }
    }
}

/// One bounded change signal per subscriber; output remains in the existing
/// sequence-addressed ring. Register before reading to avoid lost wakeups.
pub struct DesktopTerminalSubscription {
    record: Arc<Mutex<TerminalRecord>>,
    changes: tokio::sync::watch::Receiver<u64>,
    last_seq: u64,
    initial: bool,
}
impl DesktopTerminalSubscription {
    pub async fn next(&mut self) -> Result<DesktopTerminalFrame, String> {
        if !self.initial {
            self.changes
                .changed()
                .await
                .map_err(|_| "terminal subscription closed")?;
        }
        self.initial = false;
        self.changes.borrow_and_update();
        let guard = self.record.lock();
        let (text, truncated) = guard.output.collect_after(self.last_seq);
        self.last_seq = guard.output.seq;
        Ok(DesktopTerminalFrame {
            terminal_id: guard.snapshot.terminal_id.clone(),
            seq: self.last_seq,
            truncated,
            text,
            status: guard.snapshot.status,
            backend: guard.snapshot.backend,
            exit_code: guard.snapshot.exit_code,
            error: guard.snapshot.error.clone(),
        })
    }
}

pub struct DesktopTerminalService {
    terminals: DashMap<String, Arc<Mutex<TerminalRecord>>>,
    active: Arc<AtomicUsize>,
    changed: Arc<Notify>,
    transcript_storage: Option<Arc<dyn StorageBackend>>,
    /// `winpty.dll` to run shells in a real console, when the host has one.
    winpty: Option<PathBuf>,
}

impl Default for DesktopTerminalService {
    fn default() -> Self {
        Self {
            terminals: DashMap::new(),
            active: Arc::new(AtomicUsize::new(0)),
            changed: Arc::new(Notify::new()),
            transcript_storage: None,
            winpty: None,
        }
    }
}

impl DesktopTerminalService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable durable transcripts: shell output is spilled to storage in
    /// bounded chunks so a restart can restore the transcript of the scope.
    pub fn with_transcript_storage(mut self, storage: Arc<dyn StorageBackend>) -> Self {
        self.transcript_storage = Some(storage);
        self
    }

    /// Enable real consoles: shells start under `winpty` instead of three pipes.
    /// A missing or unloadable library is not an error; those shells fall back to
    /// pipes, which is what a host without the supplement package gets.
    pub fn with_winpty(mut self, dll: Option<PathBuf>) -> Self {
        self.winpty = dll;
        self
    }

    /// Restored transcript of a session scope: raw shell output of the newest
    /// stored chunks in stream order, plus the newest stored stream position.
    pub fn transcript(&self, user_id: &str, session_id: &str) -> (String, u64) {
        let Some(storage) = self.transcript_storage.as_ref() else {
            return (String::new(), 0);
        };
        // Read the tail, not the whole scope: a long-lived terminal must not
        // turn startup recovery into an unbounded render pass.
        let latest = storage
            .terminal_transcript_next_chunk(user_id, session_id)
            .unwrap_or(0);
        let from_chunk = latest.saturating_sub(TRANSCRIPT_RESTORE_CHUNKS).max(0);
        let blocks = match storage.list_terminal_transcript_blocks(
            user_id,
            session_id,
            from_chunk,
            TRANSCRIPT_RESTORE_CHUNKS,
        ) {
            Ok(blocks) => blocks,
            Err(error) => {
                tracing::warn!("terminal transcript restore failed: {error}");
                return (String::new(), 0);
            }
        };
        let mut text = String::new();
        let mut seq = 0u64;
        for block in blocks {
            if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                text.push_str(chunk);
            }
            seq = seq.max(block.get("seq").and_then(Value::as_i64).unwrap_or(0).max(0) as u64);
        }
        (text, seq)
    }

    pub fn start(&self, spec: DesktopTerminalStartSpec) -> Result<DesktopTerminalSnapshot, String> {
        self.prune_expired();
        let terminal_id = spec
            .terminal_id
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("term_{}", Uuid::new_v4().simple()));
        if let Some(existing) = self.terminals.get(&terminal_id) {
            let record = Arc::clone(existing.value());
            let snapshot = record.lock().snapshot();
            if snapshot.user_id == spec.user_id && snapshot.session_id == spec.session_id {
                return Ok(snapshot);
            }
        }
        if self.active.load(Ordering::Acquire) >= MAX_ACTIVE_TERMINALS {
            return Err("too many active terminals; close one first".to_string());
        }

        let command = spec.command.clone().unwrap_or_default().trim().to_string();
        let resolved_shell = resolve_terminal_shell(spec.shell.as_deref().unwrap_or_default());
        let shell = crate::core::command_utils::shell_display_name(&resolved_shell);

        let now = Utc::now();
        let transcript = TranscriptSink::open(
            self.transcript_storage.clone(),
            &spec.user_id,
            &spec.session_id,
        );
        let record = Arc::new(Mutex::new(TerminalRecord {
            snapshot: DesktopTerminalSnapshot {
                terminal_id: terminal_id.clone(),
                user_id: spec.user_id.clone(),
                session_id: spec.session_id.clone(),
                workspace_id: spec.workspace_id.clone(),
                command: command.clone(),
                cwd: spec.cwd.clone(),
                shell: shell.clone(),
                status: DesktopTerminalStatus::Starting,
                backend: DesktopTerminalBackend::Pipes,
                seq: 0,
                dropped_bytes: 0,
                truncated: false,
                started_at: now,
                ended_at: None,
                exit_code: None,
                error: None,
            },
            output: OutputBuffer::new(TERMINAL_OUTPUT_BUDGET_BYTES),
            input: Mutex::new(None),
            console: None,
            #[cfg(unix)]
            unix_pty: None,
            // The client reports the panel's real size on its first paint.
            size: (DEFAULT_COLS, DEFAULT_ROWS),
            cancel: CancellationToken::new(),
            transcript,
            changes: tokio::sync::watch::channel(0).0,
        }));
        self.terminals
            .insert(terminal_id.clone(), Arc::clone(&record));
        self.active.fetch_add(1, Ordering::AcqRel);

        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "no tokio runtime available".to_string())?;
        let spawn_record = Arc::clone(&record);
        let spawn_active = Arc::clone(&self.active);
        let spawn_changed = Arc::clone(&self.changed);
        let spawn_winpty = self.winpty.clone();
        runtime.spawn(async move {
            Self::spawn_and_pump(
                spawn_active,
                spawn_changed,
                terminal_id,
                spec,
                spawn_winpty,
                spawn_record,
            )
            .await;
        });
        self.changed.notify_waiters();
        let snapshot = record.lock().snapshot();
        Ok(snapshot)
    }

    /// Synchronous snapshot fetch. The frontend pulls on a timer; this call
    /// never blocks on the runtime and is safe from any thread.
    pub fn poll(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        last_seq: u64,
    ) -> Result<DesktopTerminalFrame, String> {
        self.frame_after(user_id, session_id, terminal_id, last_seq)
    }

    pub fn subscribe(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        last_seq: u64,
    ) -> Result<DesktopTerminalSubscription, String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let changes = record.lock().changes.subscribe();
        Ok(DesktopTerminalSubscription {
            record,
            changes,
            last_seq,
            initial: true,
        })
    }

    pub async fn write_stdin(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        input: &[u8],
    ) -> Result<(), String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let status = record.lock().snapshot.status;
        if status != DesktopTerminalStatus::Running && status != DesktopTerminalStatus::Starting {
            return Err("terminal has exited".to_string());
        }
        // Release the record before any I/O: a full input pipe must never
        // prevent output readers or subscribers from acquiring the record.
        let sink = {
            let guard = record.lock();
            let sink = guard.input.lock().clone();
            sink.ok_or("terminal stdin is unavailable")?
        };
        let target = match sink {
            InputSink::Child(stdin) => {
                stdin
                    .lock()
                    .await
                    .write_all(input)
                    .await
                    .map_err(|error| format!("failed to write terminal stdin: {error}"))?;
                return Ok(());
            }
            InputSink::Console(file) => BlockingTerminalIo::Console(file),
            #[cfg(unix)]
            InputSink::Pty(pty) => BlockingTerminalIo::Pty(pty),
        };
        let bytes = input.to_vec();
        tokio::task::spawn_blocking(move || {
            let result = match target {
                BlockingTerminalIo::Console(file) => std::io::Write::write_all(&mut &*file, &bytes),
                #[cfg(unix)]
                BlockingTerminalIo::Pty(pty) => pty.input.lock().write_all(&bytes),
            };
            result.map_err(|error| format!("failed to write terminal input: {error}"))
        })
        .await
        .map_err(|error| format!("terminal input task failed: {error}"))??;
        Ok(())
    }

    /// Submit an edited command line. Pipes have no terminal line discipline:
    /// they need a complete line and an explicit echo, whereas a console/PTY
    /// receives Return and performs its own echo. Keep this distinction here
    /// so native clients do not guess shell-specific stdin framing.
    pub async fn write_command(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        command: &str,
    ) -> Result<(), String> {
        if command.len() > 64 * 1024 {
            return Err("terminal command exceeds input limit".into());
        }
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let snapshot = record.lock().snapshot();
        if snapshot.status != DesktopTerminalStatus::Running {
            return Err("terminal is not ready for input".into());
        }
        #[cfg(windows)]
        let cmd_shell = resolve_terminal_shell(&snapshot.shell).kind
            == crate::core::command_utils::ShellKind::Cmd;
        #[cfg(not(windows))]
        let cmd_shell = false;
        let input = command_line_input(command, snapshot.backend, cmd_shell);
        if snapshot.backend == DesktopTerminalBackend::Pipes {
            // The echo belongs to the durable stream, not a frontend overlay.
            // Queue it before the write so output from a fast command follows it.
            let echo = format!("> {}\r\n", command.replace('\r', ""));
            record.lock().push_output(&echo, false);
        }
        self.write_stdin(user_id, session_id, terminal_id, &input)
            .await
    }

    /// Interrupt the shell. On a console this is a real Ctrl-C, which the shell
    /// turns into the signal its foreground program expects; on pipes there is no
    /// way to say that, so the process is terminated instead.
    pub fn interrupt(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<(), String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let input = {
            let guard = record.lock();
            let input_guard = guard.input.lock();
            match input_guard.as_ref() {
                Some(InputSink::Console(file)) => {
                    Some(BlockingTerminalIo::Console(Arc::clone(file)))
                }
                #[cfg(unix)]
                Some(InputSink::Pty(pty)) => Some(BlockingTerminalIo::Pty(Arc::clone(pty))),
                _ => None,
            }
        };
        if let Some(input) = input {
            // In canonical terminal mode Ctrl-C is interpreted by the PTY as
            // SIGINT for the foreground process group, while the shell stays.
            let result = match input {
                BlockingTerminalIo::Console(file) => std::io::Write::write_all(&mut &*file, &[3u8]),
                #[cfg(unix)]
                BlockingTerminalIo::Pty(pty) => pty.input.lock().write_all(&[3u8]),
            };
            return result.map_err(|error| format!("failed to send Ctrl-C: {error}"));
        }
        self.terminate(user_id, session_id, terminal_id)
    }

    /// Tell the shell its window changed size. Only a console can hear it: a
    /// piped shell has no width to be told about.
    pub fn resize(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let console = {
            let mut guard = record.lock();
            if guard.size == (cols, rows) {
                return Ok(());
            }
            guard.size = (cols, rows);
            guard.console.clone()
        };
        #[cfg(unix)]
        let unix_pty = record.lock().unix_pty.clone();
        match console {
            Some(console) => apply_console_size(&console, cols, rows),
            None => {
                #[cfg(unix)]
                if let Some(pty) = unix_pty {
                    return pty
                        .master
                        .lock()
                        .resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        })
                        .map_err(|error| format!("failed to resize terminal: {error}"));
                }
                Ok(())
            }
        }
    }

    /// Request process termination (no TTY so this is an OS-level kill, not a
    /// signal injection). The process watcher observes the exit and updates
    /// the snapshot asynchronously.
    pub fn terminate(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<(), String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        record.lock().cancel.cancel();
        self.changed.notify_waiters();
        Ok(())
    }

    pub fn close(&self, user_id: &str, session_id: &str, terminal_id: &str) -> Result<(), String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let writer = {
            let mut guard = record.lock();
            // The record is dropped right after this, so the buffered tail has
            // to reach storage before the cancel.
            guard.spill_transcript();
            guard.cancel.cancel();
            guard.snapshot.status = DesktopTerminalStatus::Exited;
            guard.changed();
            guard.transcript.writer.clone()
        };
        // Closing is a durability boundary. Wait outside all record locks.
        if let Some(writer) = writer {
            let (done, wait) = std::sync::mpsc::sync_channel(1);
            if writer.send(TranscriptWrite::Flush(done)).is_ok() {
                let _ = wait.recv();
            }
        }
        self.terminals.remove(terminal_id);
        self.changed.notify_waiters();
        Ok(())
    }

    pub fn list(&self, user_id: &str, session_id: &str) -> Vec<DesktopTerminalSnapshot> {
        self.prune_expired();
        let mut snapshots = self
            .terminals
            .iter()
            .filter_map(|entry| {
                let record = entry.value().lock();
                if record.snapshot.user_id != user_id || record.snapshot.session_id != session_id {
                    return None;
                }
                Some(record.snapshot())
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|left, right| left.started_at.cmp(&right.started_at));
        snapshots
    }

    fn frame_after(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
        last_seq: u64,
    ) -> Result<DesktopTerminalFrame, String> {
        let record = self.scoped_record(user_id, session_id, terminal_id)?;
        let guard = record.lock();
        let snapshot = guard.snapshot();
        let (text, truncated) = guard.output.collect_after(last_seq);
        Ok(DesktopTerminalFrame {
            terminal_id: terminal_id.to_string(),
            seq: snapshot.seq,
            truncated,
            text,
            status: snapshot.status,
            backend: snapshot.backend,
            exit_code: snapshot.exit_code,
            error: snapshot.error,
        })
    }

    fn scoped_record(
        &self,
        user_id: &str,
        session_id: &str,
        terminal_id: &str,
    ) -> Result<Arc<Mutex<TerminalRecord>>, String> {
        let entry = self
            .terminals
            .get(terminal_id)
            .ok_or_else(|| "unknown terminal".to_string())?;
        let record = Arc::clone(entry.value());
        let guard = record.lock();
        if guard.snapshot.user_id != user_id || guard.snapshot.session_id != session_id {
            return Err("terminal is not accessible in this scope".to_string());
        }
        drop(guard);
        Ok(record)
    }

    fn prune_expired(&self) {
        let now = Utc::now();
        let expired = self
            .terminals
            .iter()
            .filter_map(|entry| {
                let record = entry.value().lock();
                let ended = record.snapshot.ended_at;
                let is_exited = record.snapshot.status == DesktopTerminalStatus::Exited
                    || record.snapshot.status == DesktopTerminalStatus::FailedToStart;
                ended
                    .filter(|deadline| is_exited && *deadline < now - chrono::Duration::minutes(10))
                    .map(|_| entry.key().clone())
            })
            .collect::<Vec<_>>();
        for id in expired {
            self.terminals.remove(&id);
        }
    }

    /// Run a shell on a console. One thread reads what the shell draws, one
    /// waits for it to exit, and the record holds the console so the viewport
    /// can be resized and so closing the record takes the shell down with it.
    #[cfg(all(windows, feature = "terminal-pty"))]
    async fn pump_console(
        active: Arc<AtomicUsize>,
        changed: Arc<Notify>,
        _terminal_id: String,
        session: ConsoleSession,
        record: Arc<Mutex<TerminalRecord>>,
    ) {
        let ConsoleSession {
            console,
            input,
            output,
        } = session;
        let input = Arc::new(input);
        let cancel = record.lock().cancel.clone();
        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Running;
            guard.snapshot.backend = DesktopTerminalBackend::Console;
            *guard.input.lock() = Some(InputSink::Console(Arc::clone(&input)));
            guard.console = Some(Arc::clone(&console));
            guard.changed();
        }

        // A resize can arrive while CreateProcess is still starting.
        let size = record.lock().size;
        let _ = console.set_size(size.0, size.1);
        let reader_record = Arc::clone(&record);
        let reader_changed = Arc::clone(&changed);
        let reader_cancel = cancel.clone();
        let reader = tokio::task::spawn_blocking(move || {
            pump_console_output(output, reader_record, reader_changed, reader_cancel)
        });

        let wait_console = Arc::clone(&console);
        let mut wait_task = tokio::task::spawn_blocking(move || wait_console.wait());
        let exit = tokio::select! {
            result = &mut wait_task => result.unwrap_or_default(),
            _ = cancel.cancelled() => {
                // Nobody is watching any more. Closing the console stops the
                // agent, and the agent's shutdown flag stops the shell, which is
                // what releases the reader and the waiter above.
                console.shutdown();
                record.lock().console = None;
                wait_task.abort();
                None
            }
        };
        console.shutdown();
        let _ = reader.await;
        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Exited;
            guard.snapshot.ended_at = Some(Utc::now());
            guard.snapshot.exit_code = exit;
            guard.console = None;
            guard.spill_transcript();
            guard.changed();
        }
        active.fetch_sub(1, Ordering::AcqRel);
        changed.notify_waiters();
    }

    /// Unix always runs the shell on a local PTY, so it gets its own body: the
    /// console/pipes fallback below is only reachable on Windows and would be
    /// dead code (and leave the console arguments unused) on Unix.
    #[cfg(unix)]
    async fn spawn_and_pump(
        active: Arc<AtomicUsize>,
        changed: Arc<Notify>,
        _terminal_id: String,
        spec: DesktopTerminalStartSpec,
        _winpty: Option<PathBuf>,
        record: Arc<Mutex<TerminalRecord>>,
    ) {
        let size = record.lock().size;
        match open_unix_pty(&spec, size) {
            Ok(session) => Self::pump_unix_pty(active, changed, session, record).await,
            Err(error) => {
                let mut guard = record.lock();
                guard.snapshot.status = DesktopTerminalStatus::FailedToStart;
                guard.snapshot.ended_at = Some(Utc::now());
                guard.snapshot.error = Some(format!("failed to open local PTY: {error}"));
                guard.changed();
                active.fetch_sub(1, Ordering::AcqRel);
                changed.notify_waiters();
            }
        }
    }

    /// Windows prefers a console when the `terminal-pty` backend is available
    /// and falls back to pipes otherwise.
    #[cfg(not(unix))]
    async fn spawn_and_pump(
        active: Arc<AtomicUsize>,
        changed: Arc<Notify>,
        terminal_id: String,
        spec: DesktopTerminalStartSpec,
        winpty: Option<PathBuf>,
        record: Arc<Mutex<TerminalRecord>>,
    ) {
        #[cfg(all(windows, feature = "terminal-pty"))]
        {
            let size = record.lock().size;
            match open_console(winpty.as_deref(), &spec, size) {
                Ok(session) => {
                    Self::pump_console(active, changed, terminal_id, session, record).await;
                    return;
                }
                // A shell that cannot get a console still gets a shell.
                Err(error) => {
                    tracing::warn!("terminal console unavailable for {terminal_id}: {error}")
                }
            }
        }
        // Builds without a console backend always fall back to pipes, so a
        // configured console path is dead weight: record it once instead of
        // silently ignoring the two arguments.
        #[cfg(not(all(windows, feature = "terminal-pty")))]
        if winpty.is_some() {
            tracing::debug!("console backend unavailable for {terminal_id}; starting on pipes");
        }
        let mut child = match spawn_terminal_process(&spec) {
            Ok(child) => Some(child),
            Err(error) => {
                let mut guard = record.lock();
                guard.snapshot.status = DesktopTerminalStatus::FailedToStart;
                guard.snapshot.ended_at = Some(Utc::now());
                guard.snapshot.error = Some(error.clone());
                guard.changed();
                active.fetch_sub(1, Ordering::AcqRel);
                changed.notify_waiters();
                return;
            }
        };

        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Running;
            let stdin = child.as_mut().and_then(|child| child.stdin.take());
            *guard.input.lock() =
                stdin.map(|stdin| InputSink::Child(Arc::new(tokio::sync::Mutex::new(stdin))));
            guard.changed();
        }

        let stdout = child.as_mut().and_then(|child| child.stdout.take());
        let stderr = child.as_mut().and_then(|child| child.stderr.take());

        let cancel = record.lock().cancel.clone();
        let reader_changed = Arc::clone(&changed);
        let gbk_fallback = cfg!(windows);

        let mut tasks = Vec::new();
        if let Some(stdout) = stdout {
            let reader_record = Arc::clone(&record);
            let reader_changed = Arc::clone(&reader_changed);
            let reader_cancel = cancel.clone();
            tasks.push(tokio::spawn(async move {
                read_stream_into_record(
                    stdout,
                    reader_record,
                    reader_changed,
                    reader_cancel,
                    gbk_fallback,
                )
                .await
            }));
        }
        if let Some(stderr) = stderr {
            let reader_record = Arc::clone(&record);
            let reader_changed = Arc::clone(&reader_changed);
            let reader_cancel = cancel.clone();
            tasks.push(tokio::spawn(async move {
                read_stream_into_record(
                    stderr,
                    reader_record,
                    reader_changed,
                    reader_cancel,
                    gbk_fallback,
                )
                .await
            }));
        }

        let cancelled = cancel.clone();
        let mut wait_task = tokio::spawn(async move {
            match child.take() {
                Some(mut child) => child.wait().await,
                None => Err(std::io::Error::other("terminal process missing")),
            }
        });
        let exit = tokio::select! {
            result = &mut wait_task => match result {
                Ok(result) => result,
                Err(error) => Err(std::io::Error::other(error.to_string())),
            },
            _ = cancelled.cancelled() => {
                wait_task.abort();
                Err(std::io::Error::other("terminal terminated"))
            }
        };

        for task in tasks {
            let _ = task.await;
        }

        let (exit_code, error) = match exit {
            Ok(status) => (status.code(), None),
            Err(error) => (None, Some(error.to_string())),
        };

        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Exited;
            guard.snapshot.ended_at = Some(Utc::now());
            guard.snapshot.exit_code = exit_code;
            guard.snapshot.error = error;
            guard.spill_transcript();
            guard.changed();
        }
        active.fetch_sub(1, Ordering::AcqRel);
        changed.notify_waiters();
    }

    #[cfg(unix)]
    async fn pump_unix_pty(
        active: Arc<AtomicUsize>,
        changed: Arc<Notify>,
        session: UnixPtySession,
        record: Arc<Mutex<TerminalRecord>>,
    ) {
        let UnixPtySession {
            handle,
            reader,
            child,
        } = session;
        let cancel = record.lock().cancel.clone();
        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Running;
            guard.snapshot.backend = DesktopTerminalBackend::Pty;
            *guard.input.lock() = Some(InputSink::Pty(Arc::clone(&handle)));
            guard.unix_pty = Some(Arc::clone(&handle));
            guard.changed();
        }

        let reader_record = Arc::clone(&record);
        let reader_changed = Arc::clone(&changed);
        let reader = tokio::task::spawn_blocking(move || {
            pump_pty_output(reader, reader_record, reader_changed)
        });
        let wait = tokio::task::spawn_blocking(move || {
            let mut child = child;
            child.wait()
        });
        let exit = tokio::select! {
            result = wait => result.ok().and_then(Result::ok).map(|status| status.exit_code() as i32),
            _ = cancel.cancelled() => {
                let _ = handle.killer.lock().kill();
                None
            }
        };
        // The PTY reader reaches EOF/EIO when the foreground shell exits.
        // Cancellation can kill a process that leaves grandchildren behind;
        // don't keep the UI lock waiting on a blocked reader in that case.
        if exit.is_some() {
            let _ = reader.await;
        } else {
            reader.abort();
        }
        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Exited;
            guard.snapshot.ended_at = Some(Utc::now());
            guard.snapshot.exit_code = exit;
            guard.unix_pty = None;
            guard.spill_transcript();
            guard.changed();
        }
        active.fetch_sub(1, Ordering::AcqRel);
        changed.notify_waiters();
    }
}

fn command_line_input(command: &str, backend: DesktopTerminalBackend, cmd_shell: bool) -> Vec<u8> {
    let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
    let newline = match backend {
        DesktopTerminalBackend::Pipes if cmd_shell => "\r\n",
        DesktopTerminalBackend::Pipes => "\n",
        DesktopTerminalBackend::Console | DesktopTerminalBackend::Pty => "\r",
    };
    let mut input = normalized.replace('\n', newline);
    input.push_str(newline);
    input.into_bytes()
}

#[cfg(unix)]
fn open_unix_pty(
    spec: &DesktopTerminalStartSpec,
    size: (u16, u16),
) -> Result<UnixPtySession, String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: size.1.max(1),
            cols: size.0.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())?;
    let launch = resolve_terminal_shell(spec.shell.as_deref().unwrap_or_default());
    let program = launch.bash_bin.unwrap_or_else(|| PathBuf::from("bash"));
    let mut command = CommandBuilder::new(program);
    command.arg("--norc");
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.cwd(&spec.cwd);
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| error.to_string())?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    let killer = child.clone_killer();
    let handle = Arc::new(UnixPtyHandle {
        master: Mutex::new(pair.master),
        input: Mutex::new(writer),
        killer: Mutex::new(killer),
    });
    Ok(UnixPtySession {
        handle,
        reader,
        child,
    })
}

#[cfg(unix)]
fn pump_pty_output(
    mut reader: Box<dyn std::io::Read + Send>,
    record: Arc<Mutex<TerminalRecord>>,
    changed: Arc<Notify>,
) {
    use std::io::Read;
    let mut decoder = IncrementalDecoder::new(false);
    let mut buf = vec![0u8; PIPE_CHUNK_BYTES];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let text = decoder.push(&buf[..read]);
                if !text.is_empty() {
                    record.lock().push_output(&text, false);
                    changed.notify_waiters();
                }
            }
        }
    }
    record.lock().spill_transcript();
}

/// Resolve the shell launcher for a terminal based on the requested shell
/// string and the desktop runtime configuration. An empty request means
/// "auto": use the configured Git Bash when available, else the platform
/// default.
#[cfg(windows)]
fn resolve_terminal_shell(requested: &str) -> crate::core::command_utils::ShellLaunch {
    use crate::core::command_utils::{self, ShellKind, ShellLaunch};
    let lower = requested.trim().to_ascii_lowercase();
    if lower.contains("powershell") || lower == "pwsh" {
        return ShellLaunch {
            kind: ShellKind::PowerShell,
            bash_bin: None,
            path_dirs: Vec::new(),
        };
    }
    if lower.contains("bash") {
        let env = crate::core::python_runtime::resolve_desktop_command_env();
        // Bash explicitly requested; fall back to the platform default when
        // no Git Bash is available.
        return if env.shell.kind == ShellKind::Bash {
            env.shell
        } else {
            command_utils::platform_default_shell()
        };
    }
    if !lower.is_empty() {
        return ShellLaunch {
            kind: ShellKind::Cmd,
            bash_bin: None,
            path_dirs: Vec::new(),
        };
    }
    crate::core::python_runtime::resolve_desktop_command_env().shell
}

#[cfg(not(windows))]
fn resolve_terminal_shell(_requested: &str) -> crate::core::command_utils::ShellLaunch {
    crate::core::command_utils::platform_default_shell()
}

/// Prepend the Git Bash POSIX toolchain directories to PATH so the shell can
/// locate bash, ls, grep, etc. even on a stripped distribution.
#[cfg(windows)]
fn apply_terminal_path_dirs(cmd: &mut tokio::process::Command, dirs: &[PathBuf]) {
    if dirs.is_empty() {
        return;
    }
    let Some(prefix) = path_prefix(dirs) else {
        return;
    };
    let current = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{prefix};{current}"));
}

/// Build and spawn a persistent interactive shell process (stdin/stdout/stderr
/// piped). The command line itself is platform specific, see
/// [`pipe_shell_command`].
fn spawn_terminal_process(spec: &DesktopTerminalStartSpec) -> Result<Child, String> {
    let cwd = Path::new(&spec.cwd);
    let mut cmd = pipe_shell_command(spec);

    cmd.current_dir(cwd);
    crate::core::command_utils::apply_platform_spawn_options(&mut cmd);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);
    cmd.spawn()
        .map_err(|error| format!("failed to spawn terminal shell: {error}"))
}

/// Interactive shell over piped stdin. Defaults to Git Bash when configured,
/// else cmd.exe; PowerShell is used only when explicitly requested.
#[cfg(windows)]
fn pipe_shell_command(spec: &DesktopTerminalStartSpec) -> tokio::process::Command {
    use crate::core::command_utils::ShellKind;
    let resolved_shell = resolve_terminal_shell(spec.shell.as_deref().unwrap_or_default());
    match resolved_shell.kind {
        ShellKind::Bash => {
            let bash_bin = resolved_shell.bash_bin.expect("bash bin");
            let mut cmd = tokio::process::Command::new(bash_bin);
            cmd.arg("-i");
            apply_terminal_path_dirs(&mut cmd, &resolved_shell.path_dirs);
            cmd
        }
        ShellKind::PowerShell => {
            let mut cmd = tokio::process::Command::new("powershell.exe");
            cmd.arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-Command")
                .arg("-");
            cmd
        }
        ShellKind::Cmd => {
            let mut cmd = tokio::process::Command::new("cmd.exe");
            cmd.arg("/Q").arg("/D");
            cmd
        }
    }
}

#[cfg(not(windows))]
fn pipe_shell_command(spec: &DesktopTerminalStartSpec) -> tokio::process::Command {
    let launch = resolve_terminal_shell(spec.shell.as_deref().unwrap_or_default());
    let mut cmd =
        tokio::process::Command::new(launch.bash_bin.unwrap_or_else(|| PathBuf::from("bash")));
    cmd.arg("--norc");
    cmd
}

/// Start a shell on a real console instead of three pipes.
#[cfg(all(windows, feature = "terminal-pty"))]
struct ConsoleSession {
    console: Arc<Console>,
    input: File,
    output: File,
}

#[cfg(all(windows, feature = "terminal-pty"))]
fn open_console(
    dll: Option<&Path>,
    spec: &DesktopTerminalStartSpec,
    size: (u16, u16),
) -> Result<ConsoleSession, String> {
    let launch = resolve_terminal_shell(spec.shell.as_deref().unwrap_or_default());
    let cmdline = terminal_command_line(&launch);
    let env = terminal_environment(&launch.path_dirs);
    if let Ok(session) =
        terminal_conpty::spawn(&cmdline, Path::new(&spec.cwd), &env, size.0, size.1)
    {
        return Ok(ConsoleSession {
            console: Arc::new(Console::Native(session.console)),
            input: session.input,
            output: session.output,
        });
    }
    let session = terminal_pty::spawn(&terminal_pty::PtyRequest {
        dll: dll.ok_or(
            "interactive terminal requires the Git terminal supplement on this Windows version",
        )?,
        cmdline: &cmdline,
        cwd: Some(Path::new(&spec.cwd)),
        env: Some(&env),
        cols: size.0.max(1),
        rows: size.1.max(1),
    })?;
    Ok(ConsoleSession {
        console: Arc::new(Console::Legacy(session.console)),
        input: session.input,
        output: session.output,
    })
}

#[cfg(all(windows, feature = "terminal-pty"))]
fn apply_console_size(console: &Arc<Console>, cols: u16, rows: u16) -> Result<(), String> {
    console.set_size(cols, rows)
}

/// The command line `CreateProcessW` sees, mirroring the argument choices the
/// pipe path makes so a shell behaves the same either way — except for
/// PowerShell's `-Command -`, which means "read the script from redirected
/// stdin". A console has nothing redirected, and PowerShell answers that
/// contradiction by printing its own help, so there the shell starts its
/// interactive prompt instead.
#[cfg(all(windows, feature = "terminal-pty"))]
fn terminal_command_line(launch: &crate::core::command_utils::ShellLaunch) -> String {
    use crate::core::command_utils::ShellKind;
    match launch.kind {
        ShellKind::Bash => match launch.bash_bin.as_ref() {
            Some(bash) => format!("\"{}\" -i", bash.display()),
            None => "bash.exe -i".to_string(),
        },
        ShellKind::PowerShell => "powershell.exe -NoLogo -NoProfile".to_string(),
        ShellKind::Cmd => "cmd.exe /Q /D".to_string(),
    }
}

/// The environment block for a console shell. `CreateProcessW` replaces the
/// environment outright, so this carries all of it, with the POSIX toolchain
/// directories prepended to PATH exactly as the pipe path prepends them.
#[cfg(all(windows, feature = "terminal-pty"))]
fn terminal_environment(dirs: &[PathBuf]) -> Vec<(String, String)> {
    let mut env = std::env::vars().collect::<Vec<_>>();
    env.retain(|(key, _)| {
        !key.eq_ignore_ascii_case("TERM") && !key.eq_ignore_ascii_case("COLORTERM")
    });
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("COLORTERM".into(), "truecolor".into()));
    if let Some(prefix) = path_prefix(dirs) {
        match env
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
        {
            Some((_, value)) => *value = format!("{prefix};{value}"),
            None => env.push(("PATH".to_string(), prefix)),
        }
    }
    env
}

/// Tell the console its window changed size. Where there is no console backend,
/// the record still remembers the size so a later console gets it at birth.
#[cfg(not(all(windows, feature = "terminal-pty")))]
fn apply_console_size(_console: &Arc<Console>, _cols: u16, _rows: u16) -> Result<(), String> {
    Ok(())
}

/// Semicolon-joined PATH prefix, or `None` when there is nothing to prepend.
#[cfg(windows)]
fn path_prefix(dirs: &[PathBuf]) -> Option<String> {
    (!dirs.is_empty()).then(|| {
        dirs.iter()
            .map(|dir| dir.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(";")
    })
}

/// Copy the console's output into the record until the shell closes it. A console
/// has one stream, so there is no stderr to interleave, and the decoding is the
/// console transports emit UTF-8, independently of the Windows code page.
#[cfg(all(windows, feature = "terminal-pty"))]
fn pump_console_output(
    mut output: File,
    record: Arc<Mutex<TerminalRecord>>,
    changed: Arc<Notify>,
    _cancel: CancellationToken,
) {
    use std::io::Read;
    let mut decoder = IncrementalDecoder::new(false);
    let mut buf = vec![0u8; PIPE_CHUNK_BYTES];
    loop {
        match output.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let text = decoder.push(&buf[..read]);
                if !text.is_empty() {
                    record.lock().push_output(&text, false);
                    changed.notify_waiters();
                }
            }
        }
    }
    record.lock().spill_transcript();
}

async fn read_stream_into_record<R>(
    mut stream: R,
    record: Arc<Mutex<TerminalRecord>>,
    changed: Arc<Notify>,
    cancel: CancellationToken,
    gbk_fallback: bool,
) where
    R: AsyncReadExt + Unpin,
{
    let mut decoder = IncrementalDecoder::new(gbk_fallback);
    let mut buf = vec![0u8; PIPE_CHUNK_BYTES];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            result = stream.read(&mut buf) => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        let text = decoder.push(&buf[..n]);
                        if !text.is_empty() {
                            record.lock().push_output(&text, false);
                            changed.notify_waiters();
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
    // Flush the tail of the run: whatever has not reached the spill threshold
    // still belongs to the durable transcript.
    record.lock().spill_transcript();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_keeps_multibyte_boundary_across_chunks() {
        let mut decoder = IncrementalDecoder::new(false);
        // "你" is 3 UTF-8 bytes; split after the first byte.
        let byte = "你".as_bytes()[0];
        assert_eq!(decoder.push(&[byte]), "");
        let rest = &"你".as_bytes()[1..];
        let text = decoder.push(rest);
        assert_eq!(text, "你");
    }

    #[test]
    fn unpersisted_transcript_does_not_accumulate_output() {
        let mut sink = TranscriptSink::open(None, "sample", "sample");
        for _ in 0..128 {
            sink.queue(&"x".repeat(8192));
        }
        assert!(sink.pending.is_empty());
    }

    #[tokio::test]
    async fn subscription_coalesces_wakeups_without_losing_output_or_exit() {
        let service = DesktopTerminalService::new();
        let now = Utc::now();
        let record = Arc::new(Mutex::new(TerminalRecord {
            snapshot: DesktopTerminalSnapshot {
                terminal_id: "sample_terminal".into(),
                user_id: "sample".into(),
                session_id: "sample".into(),
                workspace_id: "sample".into(),
                command: String::new(),
                cwd: String::new(),
                shell: String::new(),
                status: DesktopTerminalStatus::Running,
                backend: DesktopTerminalBackend::Console,
                seq: 0,
                dropped_bytes: 0,
                truncated: false,
                started_at: now,
                ended_at: None,
                exit_code: None,
                error: None,
            },
            output: OutputBuffer::new(TERMINAL_OUTPUT_BUDGET_BYTES),
            input: Mutex::new(None),
            console: None,
            #[cfg(unix)]
            unix_pty: None,
            size: (80, 24),
            cancel: CancellationToken::new(),
            transcript: TranscriptSink::open(None, "sample", "sample"),
            changes: tokio::sync::watch::channel(0).0,
        }));
        service
            .terminals
            .insert("sample_terminal".into(), record.clone());
        let mut subscription = service
            .subscribe("sample", "sample", "sample_terminal", 0)
            .unwrap();
        assert!(subscription.next().await.unwrap().text.is_empty());
        assert!(
            tokio::time::timeout(Duration::from_millis(10), subscription.next())
                .await
                .is_err()
        );
        {
            let mut guard = record.lock();
            for _ in 0..1000 {
                guard.push_output("示例", false);
            }
        }
        let frame = tokio::time::timeout(Duration::from_millis(100), subscription.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.text, "示例".repeat(1000));
        assert!(!frame.truncated);
        // A quiet lifecycle transition must wake the reader without polling.
        {
            let mut guard = record.lock();
            guard.snapshot.status = DesktopTerminalStatus::Exited;
            guard.changed();
        }
        let frame = tokio::time::timeout(Duration::from_millis(100), subscription.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(frame.status, DesktopTerminalStatus::Exited);
        assert!(frame.text.is_empty());
    }

    #[test]
    fn edited_commands_match_terminal_line_discipline() {
        use DesktopTerminalBackend::{Console, Pipes, Pty};
        assert_eq!(
            command_line_input("echo sample", Pipes, true),
            b"echo sample\r\n"
        );
        assert_eq!(
            command_line_input("echo sample", Pipes, false),
            b"echo sample\n"
        );
        assert_eq!(
            command_line_input("echo sample", Console, true),
            b"echo sample\r"
        );
        assert_eq!(
            command_line_input("echo sample", Pty, false),
            b"echo sample\r"
        );
        assert_eq!(
            command_line_input("echo one\r\necho two", Pipes, false),
            b"echo one\necho two\n"
        );
        assert_eq!(command_line_input("", Console, false), b"\r");
    }

    #[cfg(windows)]
    #[test]
    fn decoder_handles_gbk_pair_boundary() {
        let mut decoder = IncrementalDecoder::new(true);
        // "中" in GBK is 0xD6 0xD0. Feed only the lead byte, then the tail.
        assert_eq!(decoder.push(&[0xD6]), "");
        assert_eq!(decoder.push(&[0xD0]), "中");
    }

    /// A file whose PE header claims the given machine. Only the first bytes
    /// matter to [`pe_machine`], so the rest is padding.
    fn fake_pe(machine: u16) -> Vec<u8> {
        let mut head = vec![0u8; 512];
        head[..2].copy_from_slice(b"MZ");
        head[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        head[128..132].copy_from_slice(b"PE\0\0");
        head[132..134].copy_from_slice(&machine.to_le_bytes());
        head
    }

    /// The machine a winpty of the other word size would carry.
    const FOREIGN_MACHINE: u16 = if HOST_WINPTY_MACHINE == MACHINE_I386 {
        MACHINE_AMD64
    } else {
        MACHINE_I386
    };

    #[test]
    fn winpty_needs_its_agent_and_its_architecture() {
        let root =
            std::env::temp_dir().join(format!("wunder-winpty-layout-{}", std::process::id()));
        let usable = root.join("usable");
        let alone = root.join("alone");
        let foreign = root.join("foreign");
        let garbage = root.join("garbage");
        for dir in [&usable, &alone, &foreign, &garbage] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(usable.join("winpty.dll"), fake_pe(HOST_WINPTY_MACHINE)).unwrap();
        std::fs::write(usable.join("winpty-agent.exe"), b"").unwrap();
        // Same library, but nothing to run the console.
        std::fs::write(alone.join("winpty.dll"), fake_pe(HOST_WINPTY_MACHINE)).unwrap();
        // Correct layout, wrong word size: it could never be loaded here.
        std::fs::write(foreign.join("winpty.dll"), fake_pe(FOREIGN_MACHINE)).unwrap();
        std::fs::write(foreign.join("winpty-agent.exe"), b"").unwrap();
        // Not a PE at all.
        std::fs::write(garbage.join("winpty.dll"), b"not a library").unwrap();
        std::fs::write(garbage.join("winpty-agent.exe"), b"").unwrap();
        // A foreign winpty earlier in the list must not shadow a usable one.
        assert_eq!(
            find_winpty_library(&[alone, foreign, garbage, usable.clone()]),
            Some(usable.join("winpty.dll"))
        );
        // A directory with nothing in it is not a winpty either.
        assert_eq!(find_winpty_library(&[root.join("nowhere")]), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The pipe path's `-Command -` is a redirected-stdin idiom; handing it to a
    /// shell that owns a console makes the shell print its own help instead of
    /// starting, so the console command line must not carry it.
    #[cfg(all(windows, feature = "terminal-pty"))]
    #[test]
    fn console_shells_start_interactively() {
        use crate::core::command_utils::{ShellKind, ShellLaunch};
        let launch = |kind: ShellKind| ShellLaunch {
            kind,
            bash_bin: None,
            path_dirs: Vec::new(),
        };
        assert_eq!(
            terminal_command_line(&launch(ShellKind::PowerShell)),
            "powershell.exe -NoLogo -NoProfile"
        );
        assert_eq!(
            terminal_command_line(&launch(ShellKind::Cmd)),
            "cmd.exe /Q /D"
        );
        assert_eq!(
            terminal_command_line(&launch(ShellKind::Bash)),
            "bash.exe -i"
        );
    }

    #[cfg(windows)]
    #[test]
    fn toolchain_directories_become_a_path_prefix() {
        assert_eq!(path_prefix(&[]), None);
        assert_eq!(
            path_prefix(&[PathBuf::from("C:/git/usr/bin"), PathBuf::from("D:/tools")]).unwrap(),
            "C:/git/usr/bin;D:/tools"
        );
    }

    #[test]
    fn output_buffer_tracks_seq_and_truncation() {
        let mut buffer = OutputBuffer::new(16);
        buffer.append("alpha");
        buffer.append("beta");
        let (text, truncated) = buffer.collect_after(0);
        assert_eq!(text, "alphabeta");
        assert!(!truncated);
        let (text, truncated) = buffer.collect_after(1);
        assert_eq!(text, "beta");
        assert!(!truncated);
        let (text, _) = buffer.collect_after(2);
        assert_eq!(text, "");
    }
}
