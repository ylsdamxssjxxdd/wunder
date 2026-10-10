//! Tunnel data plane: chunking, framing and pacing of a remote file read
//! (docs §4.2, §6.4, §10.1).
//!
//! Small results ride inline in `command_result`; anything above
//! [`INLINE_MAX_BYTES`] becomes a binary stream: one `stream_open`
//! `command_event` on the control queue, then chunks of
//! `stream_id(u64be) + flags(u32be) + offset(u32be) + payload`.
//!
//! The pump awaits the bounded data queue instead of buffering: at most
//! [`MAX_INFLIGHT_CHUNKS`] chunks exist beyond the socket at any moment, and
//! the 16-byte header layout is the one the server's blob store parses.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use wunder_core::interlink::{DATA_FLAG_ERROR, DATA_FLAG_LAST, FRAME_COMMAND_EVENT};

use crate::services::interlink::blob;
use crate::services::interlink::OutboundFrame;

/// Results at or below this size are base64-inlined in `command_result`.
pub const INLINE_MAX_BYTES: usize = 1024 * 1024;
/// Chunk size used when the local config does not say otherwise (docs §3.3).
pub const DEFAULT_CHUNK_KB: usize = 256;
pub const MIN_CHUNK_KB: usize = 16;
/// Upper bound: the server caps one tunnel message at 512 KiB
/// (`api::interlink_ws::WS_MAX_MESSAGE_BYTES`), so a chunk plus its header has
/// to stay clearly below that.
pub const MAX_CHUNK_KB: usize = 448;
/// Chunks allowed between the producer and the socket (docs §10.1).
pub const MAX_INFLIGHT_CHUNKS: usize = 16;
/// Documented data-plane rate limit (docs §6.4, §9.5).
pub const MAX_BYTES_PER_S: u64 = 4 * 1024 * 1024;
/// Largest file the data plane will pump in one stream.
pub const MAX_STREAM_BYTES: u64 = 64 * 1024 * 1024;

/// One planned chunk of a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    /// Byte offset of this chunk inside the file.
    pub offset: u32,
    /// Payload length of this chunk.
    pub len: usize,
    /// True for the final chunk (carries `DATA_FLAG_LAST`).
    pub last: bool,
}

impl Chunk {
    pub fn flags(&self) -> u32 {
        if self.last {
            DATA_FLAG_LAST
        } else {
            0
        }
    }
}

/// Chunk size in bytes, clamped into the protocol band.
pub fn chunk_bytes_for(kb: usize) -> usize {
    let kb = kb.clamp(MIN_CHUNK_KB, MAX_CHUNK_KB);
    kb * 1024
}

/// Plan every chunk of a `total_len` byte file. An empty file still produces
/// one zero-length final chunk, so the consumer sees `DATA_FLAG_LAST` and can
/// close the stream instead of waiting for bytes that never come.
pub fn plan(total_len: usize, chunk_bytes: usize) -> Vec<Chunk> {
    let chunk_bytes = chunk_bytes.max(1);
    if total_len == 0 {
        return vec![Chunk {
            offset: 0,
            len: 0,
            last: true,
        }];
    }
    let count = total_len.div_ceil(chunk_bytes);
    let mut chunks = Vec::with_capacity(count.min(u32::MAX as usize));
    let mut offset = 0usize;
    while offset < total_len {
        let len = (total_len - offset).min(chunk_bytes);
        let last = offset + len >= total_len;
        chunks.push(Chunk {
            offset: offset as u32,
            len,
            last,
        });
        offset += len;
    }
    chunks
}

/// Encode one chunk with the documented 16-byte header.
pub fn encode(stream_id: u64, chunk: &Chunk, payload: &[u8]) -> Vec<u8> {
    blob::encode_chunk(stream_id, chunk.flags(), chunk.offset, payload)
}

/// Encode an abort marker: zero payload, `DATA_FLAG_LAST | DATA_FLAG_ERROR`, so
/// the consumer both closes the stream and drops what it buffered.
pub fn encode_error(stream_id: u64, offset: u32) -> Vec<u8> {
    blob::encode_chunk(stream_id, DATA_FLAG_LAST | DATA_FLAG_ERROR, offset, &[])
}

/// How long the producer must wait before the next chunk to respect
/// `rate_bps`. Returns `Duration::ZERO` when the budget is unlimited.
pub fn pace_delay(len: usize, rate_bps: u64) -> Duration {
    if rate_bps == 0 || len == 0 {
        return Duration::ZERO;
    }
    let micros = (len as u64).saturating_mul(1_000_000) / rate_bps.max(1);
    Duration::from_micros(micros)
}

/// Outcome of one pump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpOutcome {    /// Every byte, including the `DATA_FLAG_LAST` chunk, was handed over.
    Completed,
    /// The queue or the socket went away: the stream is unfinished.
    Aborted,
    /// Reading the file failed; an error chunk was attempted.
    Failed,
    /// The command was cancelled by the node or by `command.cancel`.
    Cancelled,
}

/// Stream one file over the data plane.
///
/// `tx` is the connection's bounded data queue: sending awaits a free slot, so
/// at most [`MAX_INFLIGHT_CHUNKS`] chunks are ever queued beyond the socket and
/// a stalled tunnel slows the producer instead of inflating memory.
#[allow(clippy::too_many_arguments)]
pub async fn pump_file(
    tx: &mpsc::Sender<OutboundFrame>,
    cancel: &CancellationToken,
    stream_id: u64,
    path: &Path,
    chunk_bytes: usize,
    rate_bps: u64,
) -> PumpOutcome {
    let chunk_bytes = chunk_bytes.max(1);
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(_) => return PumpOutcome::Failed,
    };
    let started = Instant::now();
    let mut sent_bytes: u64 = 0;
    let mut buffer = vec![0u8; chunk_bytes];
    loop {
        if cancel.is_cancelled() {
            return PumpOutcome::Cancelled;
        }
        let read = match file.read(&mut buffer).await {
            Ok(read) => read,
            Err(_) => {
                let frame = encode_error(stream_id, sent_bytes as u32);
                let _ = tx.send(OutboundFrame::Binary(frame)).await;
                return PumpOutcome::Failed;
            }
        };
        if read == 0 {
            // EOF: close the stream with a zero-length LAST chunk.
            let frame = blob::encode_chunk(stream_id, DATA_FLAG_LAST, sent_bytes as u32, &[]);
            if tx.send(OutboundFrame::Binary(frame)).await.is_err() {
                return PumpOutcome::Aborted;
            }
            return PumpOutcome::Completed;
        }
        let chunk = Chunk {
            offset: sent_bytes as u32,
            len: read,
            last: false,
        };
        let frame = encode(stream_id, &chunk, &buffer[..read]);
        if tx.send(OutboundFrame::Binary(frame)).await.is_err() {
            return PumpOutcome::Aborted;
        }
        sent_bytes += read as u64;
        // Pacing is measured against the wall clock since the stream started,
        // so a fast socket still cannot exceed `rate_bps`.
        let allowed = rate_bps * started.elapsed().as_secs_f64() as u64;
        if rate_bps > 0 && sent_bytes > allowed {
            let wait = pace_delay((sent_bytes - allowed) as usize, rate_bps);
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = cancel.cancelled() => return PumpOutcome::Cancelled,
            }
        }
    }
}

/// Monotonic stream id source, seeded from uuid v4 so two nodes of one account
/// cannot both start at `1` inside the same server-side blob store.
fn next_stream_id() -> u64 {
    static BASE: OnceLock<u64> = OnceLock::new();
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let base = *BASE.get_or_init(|| (Uuid::new_v4().as_u128() as u64) >> 24);
    base.wrapping_add(SEQ.fetch_add(1, Ordering::AcqRel))
}

/// Content type of one workspace file by extension; anything unknown stays
/// `application/octet-stream` so a consumer never mis-renders bytes.
pub fn content_type_for(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("json") => "application/json",
        Some("md") | Some("markdown") => "text/markdown",
        Some("txt") | Some("log") => "text/plain",
        Some("csv") => "text/csv",
        Some("html") | Some("htm") => "text/html",
        Some("xml") => "text/xml",
        Some("yaml") | Some("yml") => "application/x-yaml",
        Some("zip") => "application/zip",
        Some("wasm") => "application/wasm",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// `workspace.read`: a result up to [`INLINE_MAX_BYTES`] rides base64-inline in
/// `command_result{inline, mime, max_bytes}`; anything larger opens a stream,
/// announces it with `command_event{stream_open}` and pumps bounded chunks
/// (docs §6.4).
///
/// The ceiling is the smallest of the caller's `max_bytes`, the node's own
/// `max_file_pull_mb` and the protocol stream cap - the local limit always
/// wins.
pub async fn read_command(
    spec: &super::execute::CommandSpec,
    ctx: &super::execute::ExecContext,
    writer: &Arc<crate::services::interlink::client::TunnelWriter>,
    cancel: &CancellationToken,
) -> super::execute::CommandReport {
    use super::execute::{sanitize, CommandReport};
    let Some(path) = spec
        .args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return CommandReport::failed("PATH_REQUIRED", "path is required");
    };
    let scope = ctx.scope_user(&spec.args);
    let target = match ctx.state.workspace.resolve_path(&scope, path) {
        Ok(target) => target,
        Err(err) => return CommandReport::failed("PATH_REJECTED", sanitize(&err.to_string())),
    };
    let metadata = match tokio::fs::metadata(&target).await {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return CommandReport::failed("NOT_A_FILE", "path is not a regular file"),
        Err(_) => return CommandReport::failed("PATH_NOT_FOUND", "path does not exist"),
    };
    let size = metadata.len();
    let requested = spec
        .args
        .get("max_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(ctx.max_file_pull_bytes);
    let ceiling = requested.min(ctx.max_file_pull_bytes).min(MAX_STREAM_BYTES);
    if size > ceiling {
        return CommandReport::failed(
            "PAYLOAD_TOO_LARGE",
            format!("file is {size} bytes, the local ceiling is {ceiling}"),
        );
    }
    let mime = content_type_for(&target);
    if size as usize <= INLINE_MAX_BYTES {
        let mut file = match tokio::fs::File::open(&target).await {
            Ok(file) => file,
            Err(err) => return CommandReport::failed("READ_FAILED", sanitize(&err.to_string())),
        };
        let mut buffer = Vec::with_capacity(size as usize);
        if let Err(err) = file.read_to_end(&mut buffer).await {
            return CommandReport::failed("READ_FAILED", sanitize(&err.to_string()));
        }
        if buffer.len() as u64 > ceiling {
            return CommandReport::failed("PAYLOAD_TOO_LARGE", "file grew past the ceiling");
        }
        let inline = base64::engine::general_purpose::STANDARD.encode(&buffer);
        let bytes = buffer.len();
        return CommandReport::succeeded(json!({
            "path": path,
            "size": bytes,
            "transport": "inline",
        }))
        .with_extra("inline", json!(inline))
        .with_extra("mime", json!(mime))
        .with_extra("max_bytes", json!(ceiling))
        .with_extra("size", json!(bytes));
    }

    let stream_id = next_stream_id();
    let open = json!({
        "stage": "stream_open",
        "stream_open": {
            "stream_id": stream_id,
            "size": size,
            "mime": mime,
        },
    });
    if writer
        .send_frame(FRAME_COMMAND_EVENT, Some(spec.command_id.as_str()), open)
        .await
        .is_err()
    {
        return CommandReport::canceled("tunnel closed before stream_open");
    }
    let outcome = pump_file(
        writer.data_sender(),
        cancel,
        stream_id,
        &target,
        ctx.chunk_bytes,
        ctx.rate_bps,
    )
    .await;
    match outcome {
        PumpOutcome::Completed => CommandReport::succeeded(json!({
            "path": path,
            "size": size,
            "stream_id": stream_id,
            "transport": "stream",
        }))
        .with_extra("stream_id", json!(stream_id))
        .with_extra("size", json!(size))
        .with_extra("mime", json!(mime)),
        PumpOutcome::Cancelled => CommandReport::canceled("command cancelled during transfer"),
        PumpOutcome::Aborted => CommandReport::canceled("tunnel closed during transfer"),
        PumpOutcome::Failed => CommandReport::failed("READ_FAILED", "streaming read failed"),
    }
}
