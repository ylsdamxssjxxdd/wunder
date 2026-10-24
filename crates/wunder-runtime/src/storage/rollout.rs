//! Canonical append-only rollout storage.
//!
//! A rollout is the durable source for one physical revision of a thread
//! history.  The history projector is allowed to lag this file, but it must
//! never project an item that has not been flushed here first. SQLite stores
//! rebuildable history projections; it is never a second history authority.
//!
//! The recorder owns one bounded command receiver and opens its file lazily.
//! Consequently creating or observing an empty thread has no filesystem
//! effect, and an idle recorder is parked on `recv()` rather than driven by a
//! timer.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter, SeekFrom,
};
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tracing::warn;
use wunder_core::thread_store::{RolloutId, RolloutRecord, ThreadId};

/// Matches the bounded command channel used by the Codex rollout recorder.
/// It bounds unwritten history in memory and applies backpressure to producers
/// instead of accumulating an unbounded per-thread queue.
pub const ROLLOUT_WRITER_CAPACITY: usize = 256;
pub const ROLLOUT_MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const ROLLOUT_MAX_APPEND_BYTES: usize = 4 * 1024 * 1024;
pub const ROLLOUT_QUEUED_BYTE_BUDGET: usize = 16 * 1024 * 1024;
pub const ROLLOUT_MAX_ACTIVE_WRITERS: usize = 256;

const TAIL_SCAN_BYTES: u64 = 64 * 1024;

/// The physical location and identity of a rollout file.
///
/// `thread_id` is the stable, user-visible identity.  `rollout_id` identifies
/// this immutable physical history revision and deliberately has a separate
/// type/field so a future revert cannot accidentally overwrite an old file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutDescriptor {
    pub thread_id: String,
    pub rollout_id: String,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
}

/// The core `RolloutRecord` is written as one line of canonical JSON.  Its
/// `ordinal` is the durable history cursor, not a token counter. Byte offsets
/// returned by [`AppendReceipt`] refer to the *uncompressed* representation of
/// these newline-terminated records.
pub type PersistedRolloutRecord = RolloutRecord;

/// A position that is safe for a projector or reader to consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppendReceipt {
    pub first_ordinal: u64,
    pub next_ordinal: u64,
    pub end_byte_offset: u64,
    pub written_records: usize,
}

/// Summary returned by a full-line JSONL reader.  `complete_bytes` never
/// includes a crash-truncated suffix that lacks a final newline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RolloutReadSummary {
    pub complete_records: u64,
    pub complete_bytes: u64,
    pub ignored_incomplete_tail: bool,
}

/// A recorder handle may be cloned for the code paths belonging to one
/// rollout.  There is still exactly one worker, and it is the only component
/// that owns the open file handle.
#[derive(Clone)]
pub struct RolloutRecorder {
    descriptor: RolloutDescriptor,
    commands: mpsc::Sender<RolloutCommand>,
    queued_bytes: Arc<Semaphore>,
    // A caller can bind file-root ownership to the worker's whole lifetime,
    // including the asynchronous drain after the last handle is dropped.
    _worker_lifetime: Option<Arc<std::fs::File>>,
}

/// Storage-root entry point for runtime integrations.  It owns a bounded
/// writer only for threads which actually append records.  Keeping this
/// registry separate from the filesystem means a thread catalogue or an idle
/// subscriber does not create files or start a wakeup loop.
#[derive(Clone, Debug)]
pub struct LocalRolloutStore {
    root: Arc<PathBuf>,
    recorders: Arc<tokio::sync::Mutex<HashMap<String, Arc<RolloutRecorder>>>>,
    queued_bytes: Arc<Semaphore>,
}

impl LocalRolloutStore {
    /// Opens the storage root logically.  This performs no directory scan or
    /// directory creation; the selected writer materializes its path only on
    /// the first append.
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Arc::new(root.into()),
            recorders: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            queued_bytes: Arc::new(Semaphore::new(ROLLOUT_QUEUED_BYTE_BUDGET)),
        }
    }

    pub fn root(&self) -> &Path {
        self.root.as_ref().as_path()
    }

    /// Appends one contiguous canonical batch.  A thread must retain the same
    /// current rollout id for the lifetime of this store handle; callers that
    /// perform a revert should close the old writer before activating the new
    /// rollout revision.
    pub async fn append_rollout(
        &self,
        thread_id: &ThreadId,
        rollout_id: Option<&RolloutId>,
        records: Vec<RolloutRecord>,
    ) -> Result<AppendReceipt> {
        self.recorder_for(thread_id, rollout_id)
            .await?
            .append_records(records)
            .await
    }

    /// Flushes a previously materialized writer.  Flushing an unknown/unused
    /// thread is a no-op and cannot create an empty JSONL file.
    pub async fn flush_rollout(&self, thread_id: &ThreadId) -> Result<Option<AppendReceipt>> {
        let recorder = self.recorders.lock().await.get(thread_id.as_str()).cloned();
        match recorder {
            Some(recorder) => recorder.flush().await.map(Some),
            None => Ok(None),
        }
    }

    /// Returns the known physical path only for an active writer.  Path
    /// discovery for old threads belongs to metadata/lineage state, not this
    /// idle-safe process-local registry.
    pub async fn rollout_path(&self, thread_id: &ThreadId) -> Option<PathBuf> {
        self.recorders
            .lock()
            .await
            .get(thread_id.as_str())
            .map(|recorder| recorder.descriptor().path.clone())
    }

    /// Flushes and removes the active writer.  Removal ensures an abandoned
    /// registry entry cannot become an unbounded ThreadId-to-lock map.
    pub async fn shutdown_rollout(&self, thread_id: &ThreadId) -> Result<Option<AppendReceipt>> {
        // Retain the entry until shutdown acknowledges its file barrier, so a
        // competing append cannot create a second writer during shutdown.
        let Some(recorder) = self.recorders.lock().await.get(thread_id.as_str()).cloned() else {
            return Ok(None);
        };
        let result = recorder.shutdown().await;
        let mut recorders = self.recorders.lock().await;
        if recorders
            .get(thread_id.as_str())
            .is_some_and(|current| Arc::ptr_eq(current, &recorder))
        {
            recorders.remove(thread_id.as_str());
        }
        result.map(Some)
    }

    /// Registers metadata's known physical rollout without scanning history
    /// roots or choosing a new filename. A subsequent flush recovers its last
    /// full-line position; a missing file remains absent until a real append.
    pub async fn resume_rollout(&self, descriptor: RolloutDescriptor) -> Result<()> {
        let mut recorders = self.recorders.lock().await;
        if let Some(existing) = recorders.get(&descriptor.thread_id) {
            if existing.descriptor() != &descriptor {
                return Err(anyhow!(
                    "thread already has a different active rollout descriptor"
                ));
            }
            return Ok(());
        }
        if recorders.len() >= ROLLOUT_MAX_ACTIVE_WRITERS {
            return Err(anyhow!("active rollout writer limit reached"));
        }
        let recorder = Arc::new(RolloutRecorder::from_descriptor_with_budget(
            descriptor.clone(),
            Arc::clone(&self.queued_bytes),
        )?);
        recorders.insert(descriptor.thread_id, recorder);
        Ok(())
    }

    async fn recorder_for(
        &self,
        thread_id: &ThreadId,
        rollout_id: Option<&RolloutId>,
    ) -> Result<Arc<RolloutRecorder>> {
        let requested = rollout_id
            .map(RolloutId::as_str)
            .unwrap_or_else(|| thread_id.as_str());
        let mut recorders = self.recorders.lock().await;
        if let Some(existing) = recorders.get(thread_id.as_str()) {
            if existing.descriptor().rollout_id != requested {
                return Err(anyhow!(
                    "thread {} already has active rollout {}; shut it down before switching to {}",
                    thread_id,
                    existing.descriptor().rollout_id,
                    requested
                ));
            }
            return Ok(Arc::clone(existing));
        }
        if recorders.len() >= ROLLOUT_MAX_ACTIVE_WRITERS {
            return Err(anyhow!("active rollout writer limit reached"));
        }
        let recorder =
            RolloutRecorder::create(self.root.as_ref().as_path(), thread_id.as_str(), requested)?;
        // Replace the otherwise independent recorder budget with this store's
        // shared bound so opening many active threads cannot multiply queued
        // serialized history beyond the storage-root budget.
        let recorder = Arc::new(RolloutRecorder {
            queued_bytes: Arc::clone(&self.queued_bytes),
            ..recorder
        });
        recorders.insert(thread_id.as_str().to_owned(), Arc::clone(&recorder));
        Ok(recorder)
    }
}

impl std::fmt::Debug for RolloutRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RolloutRecorder")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl RolloutRecorder {
    /// Creates a recorder without touching the filesystem.  The first
    /// nonempty [`append_records`](Self::append_records) command creates the dated
    /// directory and file.
    pub fn create(
        storage_root: impl AsRef<Path>,
        thread_id: impl Into<String>,
        rollout_id: impl Into<String>,
    ) -> Result<Self> {
        let thread_id = checked_id(thread_id.into(), "thread id")?;
        let rollout_id = checked_id(rollout_id.into(), "rollout id")?;
        let created_at = Utc::now();
        let date_path = created_at.format("%Y/%m/%d").to_string();
        let stamp = created_at.format("%Y-%m-%dT%H-%M-%S%.3fZ");
        let filename = format!(
            "rollout-{stamp}-{}_{}.jsonl",
            safe_filename_component(&thread_id),
            safe_filename_component(&rollout_id)
        );
        let descriptor = RolloutDescriptor {
            thread_id,
            rollout_id,
            path: storage_root
                .as_ref()
                .join("sessions")
                .join(date_path)
                .join(filename),
            created_at,
        };
        Self::from_descriptor(descriptor)
    }

    /// Reopens a known physical rollout.  The file is still not opened until
    /// an append occurs.  This makes it possible for the state store to retain
    /// an archived/read-only descriptor without causing writes merely by
    /// listing threads.
    pub fn resume(descriptor: RolloutDescriptor) -> Result<Self> {
        let descriptor = RolloutDescriptor {
            thread_id: checked_id(descriptor.thread_id, "thread id")?,
            rollout_id: checked_id(descriptor.rollout_id, "rollout id")?,
            path: descriptor.path,
            created_at: descriptor.created_at,
        };
        Self::from_descriptor(descriptor)
    }

    pub(crate) fn resume_with_budget(
        descriptor: RolloutDescriptor,
        queued_bytes: Arc<Semaphore>,
        root_owner: Arc<std::fs::File>,
    ) -> Result<Self> {
        checked_id(descriptor.thread_id.clone(), "thread id")?;
        checked_id(descriptor.rollout_id.clone(), "rollout id")?;
        Self::from_descriptor_with_lifetime(descriptor, queued_bytes, Some(root_owner))
    }

    fn from_descriptor(descriptor: RolloutDescriptor) -> Result<Self> {
        Self::from_descriptor_with_budget(
            descriptor,
            Arc::new(Semaphore::new(ROLLOUT_QUEUED_BYTE_BUDGET)),
        )
    }

    fn from_descriptor_with_budget(
        descriptor: RolloutDescriptor,
        queued_bytes: Arc<Semaphore>,
    ) -> Result<Self> {
        Self::from_descriptor_with_lifetime(descriptor, queued_bytes, None)
    }

    fn from_descriptor_with_lifetime(
        descriptor: RolloutDescriptor,
        queued_bytes: Arc<Semaphore>,
        root_owner: Option<Arc<std::fs::File>>,
    ) -> Result<Self> {
        checked_id(descriptor.thread_id.clone(), "thread id")?;
        checked_id(descriptor.rollout_id.clone(), "rollout id")?;
        let (commands, receiver) = mpsc::channel(ROLLOUT_WRITER_CAPACITY);
        let worker_descriptor = descriptor.clone();
        let worker_lifetime = root_owner.clone();
        super::runtime::runtime()?.spawn(async move {
            let _owner = worker_lifetime;
            run_writer(worker_descriptor, receiver).await
        });
        Ok(Self {
            descriptor,
            commands,
            queued_bytes,
            _worker_lifetime: root_owner,
        })
    }

    pub fn descriptor(&self) -> &RolloutDescriptor {
        &self.descriptor
    }

    /// Persists a nonempty batch in command order. Admission awaits both the
    /// byte budget and the 256-command bound, applying real async backpressure.
    /// Each record is at most 8 MiB; a batch is at most 4 MiB, except a single
    /// large record which may use the full 8 MiB record limit.
    pub async fn append_records(&self, records: Vec<RolloutRecord>) -> Result<AppendReceipt> {
        if records.is_empty() {
            return Err(anyhow!("cannot append an empty rollout batch"));
        }
        let (size, record_sizes) = checked_append_size(&records)?;
        let byte_permit = Arc::clone(&self.queued_bytes)
            .acquire_many_owned(u32::try_from(size).context("rollout append size overflow")?)
            .await
            .map_err(|_| anyhow!("rollout byte budget is closed"))?;
        let mut lines = Vec::with_capacity(records.len());
        for (record, size) in records.into_iter().zip(record_sizes) {
            let mut bytes = Vec::with_capacity(size);
            serde_json::to_writer(&mut bytes, &record).context("serialize rollout record")?;
            bytes.push(b'\n');
            lines.push(PreparedLine {
                ordinal: record.ordinal,
                bytes,
            });
        }
        let (reply, response) = oneshot::channel();
        self.enqueue(RolloutCommand::Append {
            lines,
            _byte_permit: byte_permit,
            reply,
        })
        .await?;
        response
            .await
            .map_err(|_| anyhow!("rollout writer stopped before append completed"))?
            .map_err(anyhow::Error::msg)
    }

    /// Establishes a flush barrier without creating an unused empty file.
    /// The returned receipt is the last safe file position for a projector.
    pub async fn flush(&self) -> Result<AppendReceipt> {
        let (reply, response) = oneshot::channel();
        self.enqueue(RolloutCommand::Flush { reply }).await?;
        response
            .await
            .map_err(|_| anyhow!("rollout writer stopped before flush completed"))?
            .map_err(anyhow::Error::msg)
    }

    /// Flushes the current file, then terminates the sole writer.  Further
    /// calls through any cloned handle fail instead of silently reopening a
    /// second concurrent writer.
    pub async fn shutdown(&self) -> Result<AppendReceipt> {
        let (reply, response) = oneshot::channel();
        self.enqueue(RolloutCommand::Shutdown { reply }).await?;
        response
            .await
            .map_err(|_| anyhow!("rollout writer stopped before shutdown completed"))?
            .map_err(anyhow::Error::msg)
    }

    async fn enqueue(&self, command: RolloutCommand) -> Result<()> {
        self.commands
            .send(command)
            .await
            .map_err(|_| anyhow!("rollout writer is closed"))
    }
}

struct PreparedLine {
    ordinal: u64,
    bytes: Vec<u8>,
}

enum RolloutCommand {
    Append {
        lines: Vec<PreparedLine>,
        _byte_permit: OwnedSemaphorePermit,
        reply: oneshot::Sender<std::result::Result<AppendReceipt, String>>,
    },
    Flush {
        reply: oneshot::Sender<std::result::Result<AppendReceipt, String>>,
    },
    Shutdown {
        reply: oneshot::Sender<std::result::Result<AppendReceipt, String>>,
    },
}

struct WriterState {
    descriptor: RolloutDescriptor,
    file: Option<BufWriter<File>>,
    // Fields drop in declaration order: close the writable representation
    // before releasing the publication guard.
    representation_guard: Option<std::fs::File>,
    next_ordinal: u64,
    end_byte_offset: u64,
    existing_position_loaded: bool,
}

// `OwnedSemaphorePermit` lives in the Append command until that command has
// written or failed. A producer cancelled while awaiting queue capacity drops
// its complete command and promptly returns those bytes to the shared budget.

impl WriterState {
    fn empty(descriptor: RolloutDescriptor) -> Self {
        Self {
            descriptor,
            file: None,
            representation_guard: None,
            next_ordinal: 0,
            end_byte_offset: 0,
            existing_position_loaded: false,
        }
    }

    fn receipt(&self, first_ordinal: u64, written_records: usize) -> AppendReceipt {
        AppendReceipt {
            first_ordinal,
            next_ordinal: self.next_ordinal,
            end_byte_offset: self.end_byte_offset,
            written_records,
        }
    }

    async fn ensure_open(&mut self, create: bool) -> Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        let parent = self
            .descriptor
            .path
            .parent()
            .ok_or_else(|| anyhow!("rollout path has no parent directory"))?;
        if create {
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create rollout directory {}", parent.display()))?;
        }
        let path = self.descriptor.path.clone();
        let guard = crate::core::blocking::run_fs("rollout.prepare_write", move || {
            use super::rollout_compression::{
                compressed_path, materialize_rollout_locked, try_representation_lock,
            };
            if !create && !path.try_exists()? && !compressed_path(&path).try_exists()? {
                return Ok(None);
            }
            let guard = try_representation_lock(&path)?
                .ok_or_else(|| anyhow!("rollout has another writer or maintenance owner"))?;
            materialize_rollout_locked(&path)?;
            Ok(Some(guard))
        })
        .await?;
        if guard.is_none() {
            self.existing_position_loaded = true;
            return Ok(());
        }
        let mut file = match OpenOptions::new()
            .create(create)
            .read(true)
            .write(true)
            .open(&self.descriptor.path)
            .await
        {
            Ok(file) => file,
            Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
                self.existing_position_loaded = true;
                return Ok(());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("open rollout {}", self.descriptor.path.display()));
            }
        };
        let end_byte_offset = recover_complete_tail(&mut file).await?;
        let next_ordinal = last_complete_ordinal(&mut file, end_byte_offset).await?;
        file.seek(SeekFrom::Start(end_byte_offset)).await?;
        self.end_byte_offset = end_byte_offset;
        self.next_ordinal = next_ordinal;
        self.file = Some(BufWriter::new(file));
        self.representation_guard = guard;
        self.existing_position_loaded = true;
        Ok(())
    }

    async fn append(&mut self, lines: Vec<PreparedLine>) -> Result<AppendReceipt> {
        // Recover the persisted cursor before accepting caller ordinals.  A
        // resumed process must never append an apparently valid `0`-based
        // batch after an existing rollout merely because its in-memory state
        // started empty.
        self.ensure_open(true).await?;
        if self.end_byte_offset == 0 {
            if let Some(line) = lines.first() {
                let initial: PersistedRolloutRecord = serde_json::from_slice(&line.bytes)?;
                if let wunder_core::thread_store::CanonicalRecord::SessionMeta(meta) =
                    &initial.record
                {
                    let start = meta
                        .history_base
                        .as_ref()
                        .map(|base| base.end_ordinal_exclusive)
                        .unwrap_or(0);
                    if initial.ordinal != start {
                        return Err(anyhow!(
                            "session metadata ordinal does not match its lineage base"
                        ));
                    }
                    self.next_ordinal = start;
                }
            }
        }
        let first_ordinal = self.next_ordinal;
        let mut next_ordinal = self.next_ordinal;
        for line in &lines {
            if line.ordinal != next_ordinal {
                return Err(anyhow!(
                    "rollout ordinal is not contiguous: expected {}, got {}",
                    next_ordinal,
                    line.ordinal
                ));
            }
            next_ordinal = next_ordinal
                .checked_add(1)
                .ok_or_else(|| anyhow!("rollout ordinal overflow"))?;
        }
        for line in lines {
            let line_len =
                u64::try_from(line.bytes.len()).context("rollout record is too large")?;
            let next_offset = self
                .end_byte_offset
                .checked_add(line_len)
                .ok_or_else(|| anyhow!("rollout byte offset overflow"))?;
            let file = self.file.as_mut().expect("ensure_open initializes file");
            file.write_all(&line.bytes)
                .await
                .context("write rollout record")?;
            // This is intentionally a per-record flush barrier.  It does not
            // claim an fsync/sudden-power-loss durability guarantee.
            file.flush().await.context("flush rollout record")?;
            self.next_ordinal = self
                .next_ordinal
                .checked_add(1)
                .ok_or_else(|| anyhow!("rollout ordinal overflow"))?;
            self.end_byte_offset = next_offset;
        }
        Ok(self.receipt(first_ordinal, (self.next_ordinal - first_ordinal) as usize))
    }

    async fn flush(&mut self) -> Result<AppendReceipt> {
        if !self.existing_position_loaded {
            self.ensure_open(false).await?;
        }
        if let Some(file) = &mut self.file {
            file.flush().await.context("flush rollout")?;
        }
        Ok(self.receipt(self.next_ordinal, 0))
    }
}

async fn run_writer(descriptor: RolloutDescriptor, mut receiver: mpsc::Receiver<RolloutCommand>) {
    let mut state = WriterState::empty(descriptor);
    while let Some(command) = receiver.recv().await {
        match command {
            RolloutCommand::Append {
                lines,
                _byte_permit,
                reply,
            } => {
                let result = state.append(lines).await.map_err(format_error);
                let failed = result.is_err();
                if failed {
                    drop(state.file.take());
                    drop(state.representation_guard.take());
                }
                let _ = reply.send(result);
                drop(_byte_permit);
                if failed {
                    // A failed write/flush can have left a partial suffix. Do
                    // not retry later commands against uncertain file state.
                    // Resumption must explicitly recover the complete prefix.
                    receiver.close();
                    return;
                }
            }
            RolloutCommand::Flush { reply } => {
                let result = state.flush().await.map_err(format_error);
                let failed = result.is_err();
                if failed {
                    drop(state.file.take());
                    drop(state.representation_guard.take());
                }
                let _ = reply.send(result);
                if failed {
                    receiver.close();
                    return;
                }
            }
            RolloutCommand::Shutdown { reply } => {
                let result = state.flush().await.map_err(format_error);
                drop(state.file.take());
                drop(state.representation_guard.take());
                // Buffered commands can come only from direct recorder
                // clients; the coordinator serializes lifecycle boundaries.
                // Reject and release them before acknowledging ownership.
                receiver.close();
                while receiver.try_recv().is_ok() {}
                let _ = reply.send(result);
                return;
            }
        }
    }

    // Dropping all handles is also a clean lifecycle boundary.  Do not create
    // a file here: only flush one that was opened by a real append.
    if let Err(err) = state.flush().await {
        warn!("flush rollout while dropping writer failed: {err:#}");
    }
}

fn format_error(error: anyhow::Error) -> String {
    format!("{error:#}")
}

/// Counts JSON bytes without allocating another complete copy. Serialization
/// stops at the record limit, so a giant payload cannot temporarily allocate
/// an unbounded Vec before the queued-byte semaphore is acquired.
fn checked_append_size(records: &[RolloutRecord]) -> Result<(usize, Vec<usize>)> {
    let mut total = 0_usize;
    let mut sizes = Vec::with_capacity(records.len().min(4096));
    for record in records {
        let mut counter = LimitedJsonCounter { bytes: 1 }; // final newline
        serde_json::to_writer(&mut counter, record)
            .context("rollout record exceeds the 8 MiB serialized limit")?;
        total = total
            .checked_add(counter.bytes)
            .ok_or_else(|| anyhow!("rollout append size overflow"))?;
        if records.len() > 1 && total > ROLLOUT_MAX_APPEND_BYTES {
            return Err(anyhow!(
                "rollout append exceeds the 4 MiB batch limit; send a large record alone"
            ));
        }
        sizes.push(counter.bytes);
    }
    Ok((total, sizes))
}

struct LimitedJsonCounter {
    bytes: usize,
}

impl std::io::Write for LimitedJsonCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .filter(|bytes| *bytes <= ROLLOUT_MAX_RECORD_BYTES)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "serialized rollout record exceeds size limit",
                )
            })?;
        self.bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Visits only complete newline-terminated JSONL records.  A partial tail is
/// deliberately ignored: a crash after a partial write must not be presented
/// as a malformed historical event.  Opening a recorder for append later
/// removes that tail before writing the next full line.
pub async fn visit_complete_records<F>(
    path: impl AsRef<Path>,
    after_ordinal: Option<u64>,
    mut visitor: F,
) -> Result<RolloutReadSummary>
where
    F: FnMut(PersistedRolloutRecord) -> Result<()>,
{
    let read_path = path.as_ref().to_owned();
    let file = match crate::core::blocking::run_fs("rollout.open_reader", move || {
        super::rollout_compression::open_rollout_reader(read_path).map(|reader| reader.into_file())
    })
    .await
    {
        Ok(file) => File::from_std(file),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(RolloutReadSummary::default())
        }
        Err(error) => return Err(error).context("open rollout for reading"),
    };
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    let mut summary = RolloutReadSummary::default();
    loop {
        bytes.clear();
        let count = (&mut reader)
            .take((ROLLOUT_MAX_RECORD_BYTES as u64) + 1)
            .read_until(b'\n', &mut bytes)
            .await
            .context("read rollout record")?;
        if count == 0 {
            break;
        }
        if count > ROLLOUT_MAX_RECORD_BYTES {
            return Err(anyhow!("rollout record exceeds the 8 MiB serialized limit"));
        }
        if bytes.last() != Some(&b'\n') {
            summary.ignored_incomplete_tail = true;
            break;
        }
        let record: PersistedRolloutRecord = serde_json::from_slice(&bytes[..bytes.len() - 1])
            .context("parse complete rollout JSONL record")?;
        summary.complete_records = summary
            .complete_records
            .checked_add(1)
            .ok_or_else(|| anyhow!("rollout record count overflow"))?;
        summary.complete_bytes = summary
            .complete_bytes
            .checked_add(u64::try_from(count).context("rollout record size overflow")?)
            .ok_or_else(|| anyhow!("rollout byte count overflow"))?;
        if after_ordinal.is_none_or(|ordinal| record.ordinal > ordinal) {
            visitor(record)?;
        }
    }
    Ok(summary)
}

/// Truncates a crash-interrupted suffix to the last full line and returns the
/// next safe byte offset.  It scans backwards in fixed blocks, so recovery does
/// not load a large history merely to repair its final record.
async fn recover_complete_tail(file: &mut File) -> Result<u64> {
    let len = file.metadata().await?.len();
    if len == 0 {
        return Ok(0);
    }
    let mut upper = len;
    loop {
        let lower = upper.saturating_sub(TAIL_SCAN_BYTES);
        let chunk_len = usize::try_from(upper - lower).context("rollout tail scan too large")?;
        let mut chunk = vec![0_u8; chunk_len];
        file.seek(SeekFrom::Start(lower)).await?;
        file.read_exact(&mut chunk).await?;
        if let Some(index) = chunk.iter().rposition(|byte| *byte == b'\n') {
            let complete_end = lower
                .checked_add(u64::try_from(index + 1).context("rollout newline offset overflow")?)
                .ok_or_else(|| anyhow!("rollout newline offset overflow"))?;
            if complete_end != len {
                file.set_len(complete_end).await?;
            }
            return Ok(complete_end);
        }
        if lower == 0 {
            // No complete record exists.  The only safe recovery is an empty
            // file; keeping a partial first line would make the next append
            // ambiguous.
            file.set_len(0).await?;
            return Ok(0);
        }
        upper = lower;
    }
}

/// Finds the final complete JSONL line without scanning the whole history.
/// The buffer grows only with the final record, which is needed to validate
/// and recover its ordinal after a process restart.
async fn last_complete_ordinal(file: &mut File, complete_end: u64) -> Result<u64> {
    if complete_end == 0 {
        return Ok(0);
    }
    debug_assert!(complete_end > 0);
    let mut upper = complete_end - 1; // exclude the final newline itself
    let mut later_chunks: Vec<Vec<u8>> = Vec::new();
    loop {
        let lower = upper.saturating_sub(TAIL_SCAN_BYTES);
        let chunk_len = usize::try_from(upper - lower).context("rollout tail scan too large")?;
        let mut chunk = vec![0_u8; chunk_len];
        file.seek(SeekFrom::Start(lower)).await?;
        file.read_exact(&mut chunk).await?;
        if let Some(index) = chunk.iter().rposition(|byte| *byte == b'\n') {
            let mut line = chunk.split_off(index + 1);
            for later in later_chunks.iter().rev() {
                line.extend_from_slice(later);
            }
            return parse_next_ordinal(&line);
        }
        later_chunks.push(chunk);
        if later_chunks.iter().map(Vec::len).sum::<usize>() >= ROLLOUT_MAX_RECORD_BYTES {
            return Err(anyhow!(
                "final rollout record exceeds the 8 MiB serialized limit"
            ));
        }
        if lower == 0 {
            let mut line = Vec::new();
            for later in later_chunks.iter().rev() {
                line.extend_from_slice(later);
            }
            return parse_next_ordinal(&line);
        }
        upper = lower;
    }
}

fn parse_next_ordinal(line: &[u8]) -> Result<u64> {
    let record: PersistedRolloutRecord =
        serde_json::from_slice(line).context("parse final complete rollout record")?;
    record
        .ordinal
        .checked_add(1)
        .ok_or_else(|| anyhow!("rollout ordinal overflow"))
}

fn checked_id(value: String, label: &str) -> Result<String> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
        return Err(anyhow!(
            "{label} must be nonblank, NUL-free and at most 512 bytes"
        ));
    }
    Ok(value)
}

/// Uses no caller-controlled path fragment.  Thread/Rollout IDs are opaque at
/// the core boundary and therefore may contain values which are valid IDs but
/// unsafe or awkward filenames on a local platform.  The stable digest keeps
/// the path deterministic; the exact identifiers remain inside canonical
/// records and state metadata.
fn safe_filename_component(id: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut digest = Sha256::new();
    digest.update(id.as_bytes());
    let digest = format!("{:x}", digest.finalize());
    let readable: String = id
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .take(24)
        .collect();
    let readable = if readable.is_empty() { "id" } else { &readable };
    format!("{readable}-{}", &digest[..16])
}

#[cfg(test)]
mod tests {
    use super::{
        visit_complete_records, PersistedRolloutRecord, RolloutRecorder, ROLLOUT_WRITER_CAPACITY,
    };
    use serde_json::json;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;
    use wunder_core::thread_store::{
        CanonicalEvent, CanonicalItem, CanonicalItemKind, ItemId, RolloutRecord, TurnId,
    };

    fn completed_record(ordinal: u64, n: u64) -> RolloutRecord {
        RolloutRecord {
            timestamp_ms: 0,
            ordinal,
            record: wunder_core::thread_store::CanonicalRecord::Event(
                CanonicalEvent::ItemCompleted(CanonicalItem {
                    item_id: ItemId::new(format!("item_{n}")).expect("item id"),
                    turn_id: TurnId::new("turn_a").expect("turn id"),
                    root_turn_id: None,
                    created_at_ms: 0,
                    kind: CanonicalItemKind::AssistantMessage,
                    payload: json!({"n": n}),
                    display_output: None,
                    display_output_truncated: false,
                }),
            ),
        }
    }

    #[tokio::test]
    async fn empty_recorder_never_creates_a_file() {
        let root = tempdir().expect("temporary root");
        let recorder =
            RolloutRecorder::create(root.path(), "thread_a", "rollout_a").expect("create recorder");
        assert!(!recorder.descriptor().path.exists());
        recorder.shutdown().await.expect("shutdown empty recorder");
        assert!(!recorder.descriptor().path.exists());
    }

    #[tokio::test]
    async fn resumed_flush_recovers_position_without_creating_missing_file() {
        let root = tempdir().expect("temporary root");
        let recorder =
            RolloutRecorder::create(root.path(), "thread_a", "rollout_a").expect("create recorder");
        recorder
            .append_records(vec![completed_record(0, 1)])
            .await
            .expect("append record");
        let descriptor = recorder.descriptor().clone();
        let written = recorder.shutdown().await.expect("shutdown recorder");
        let resumed = RolloutRecorder::resume(descriptor).expect("resume recorder");
        let recovered = resumed.flush().await.expect("recover existing position");
        assert_eq!(recovered.next_ordinal, written.next_ordinal);
        assert_eq!(recovered.end_byte_offset, written.end_byte_offset);
        resumed.shutdown().await.expect("shutdown resumed recorder");

        let unused = RolloutRecorder::create(root.path(), "thread_b", "rollout_b")
            .expect("create unused recorder");
        let missing = unused.descriptor().clone();
        unused.shutdown().await.expect("shutdown unused recorder");
        let resumed_missing =
            RolloutRecorder::resume(missing.clone()).expect("resume missing descriptor");
        assert_eq!(
            resumed_missing
                .flush()
                .await
                .expect("flush missing rollout")
                .next_ordinal,
            0
        );
        resumed_missing
            .shutdown()
            .await
            .expect("shutdown missing rollout");
        assert!(!missing.path.exists());
    }

    #[test]
    fn oversized_append_is_rejected_before_serialized_allocation() {
        let mut record = completed_record(0, 1);
        if let wunder_core::thread_store::CanonicalRecord::Event(CanonicalEvent::ItemCompleted(
            item,
        )) = &mut record.record
        {
            item.payload = json!({"text": "a".repeat(super::ROLLOUT_MAX_RECORD_BYTES)});
        }
        assert!(super::checked_append_size(&[record]).is_err());
    }

    #[tokio::test]
    async fn appends_newline_delimited_records_in_order() {
        let root = tempdir().expect("temporary root");
        let recorder =
            RolloutRecorder::create(root.path(), "thread_a", "rollout_a").expect("create recorder");
        let receipt = recorder
            .append_records(vec![completed_record(0, 1), completed_record(1, 2)])
            .await
            .expect("append records");
        assert_eq!(receipt.first_ordinal, 0);
        assert_eq!(receipt.next_ordinal, 2);
        assert!(receipt.end_byte_offset > 2);

        let mut records = Vec::new();
        let summary = visit_complete_records(recorder.descriptor().path.clone(), None, |record| {
            records.push(record);
            Ok(())
        })
        .await
        .expect("read records");
        assert_eq!(summary.complete_records, 2);
        assert!(!summary.ignored_incomplete_tail);
        assert_eq!(records[0].ordinal, 0);
        assert_eq!(records[1].ordinal, 1);
        assert_eq!(records[1].ordinal, 1);
        recorder.shutdown().await.expect("shutdown recorder");
    }

    #[tokio::test]
    async fn reader_ignores_partial_tail_and_resume_repairs_it() {
        let root = tempdir().expect("temporary root");
        let recorder =
            RolloutRecorder::create(root.path(), "thread_a", "rollout_a").expect("create recorder");
        recorder
            .append_records(vec![completed_record(0, 1)])
            .await
            .expect("append record");
        let descriptor = recorder.descriptor().clone();
        recorder.shutdown().await.expect("shutdown recorder");

        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&descriptor.path)
            .await
            .expect("open test rollout");
        file.write_all(b"{\"ordinal\": 1")
            .await
            .expect("append partial tail");
        file.flush().await.expect("flush partial tail");
        drop(file);

        let mut before_resume = Vec::new();
        let summary = visit_complete_records(&descriptor.path, None, |record| {
            before_resume.push(record);
            Ok(())
        })
        .await
        .expect("read partial rollout");
        assert_eq!(before_resume.len(), 1);
        assert!(summary.ignored_incomplete_tail);

        let resumed = RolloutRecorder::resume(descriptor.clone()).expect("resume recorder");
        let receipt = resumed
            .append_records(vec![completed_record(1, 2)])
            .await
            .expect("append after repair");
        assert_eq!(receipt.first_ordinal, 1);
        resumed.shutdown().await.expect("shutdown resumed recorder");

        let mut after_resume = Vec::new();
        visit_complete_records(&descriptor.path, None, |record| {
            after_resume.push(record);
            Ok(())
        })
        .await
        .expect("read repaired rollout");
        assert_eq!(after_resume.len(), 2);
        assert_eq!(
            after_resume
                .iter()
                .map(|record| record.ordinal)
                .collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn channel_capacity_is_deliberate_and_nonzero() {
        assert_eq!(ROLLOUT_WRITER_CAPACITY, 256);
    }

    #[test]
    fn persisted_record_remains_json_serializable() {
        let record: PersistedRolloutRecord = completed_record(0, 1);
        assert!(serde_json::to_string(&record).is_ok());
    }
}
