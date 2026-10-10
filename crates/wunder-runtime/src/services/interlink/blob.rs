//! Tunnel data plane: large remote file reads (docs §6.4, §10.1).
//!
//! Small results ride inline in `command_result`; bigger ones are streamed as
//! binary frames (`stream_id|flags|offset` + payload) and assembled here. The
//! buffer is strictly bounded and time-limited - nothing settles in the
//! database (docs §9.3 2).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Concurrent inbound streams across the whole server.
pub const MAX_ACTIVE_STREAMS: usize = 16;
/// Largest single assembled file.
pub const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;
/// Budget for the whole keep-ready cache.
pub const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
/// How long a finished blob stays fetchable (docs §6.4: TTL 10min).
pub const BLOB_TTL_S: f64 = 600.0;

/// Binary frame header size (docs §4.2).
pub const HEADER_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    UnknownStream,
    OutOfOrder,
    TooLarge,
    TooManyStreams,
    CacheFull,
}

impl StreamError {
    pub fn code(self) -> &'static str {
        match self {
            StreamError::UnknownStream => "UNKNOWN_STREAM",
            StreamError::OutOfOrder => "CHUNK_OUT_OF_ORDER",
            StreamError::TooLarge => "FILE_TOO_LARGE",
            StreamError::TooManyStreams => "STREAM_LIMIT",
            StreamError::CacheFull => "CACHE_FULL",
        }
    }
}

/// Parsed header of one binary data frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataHeader {
    pub stream_id: u64,
    pub flags: u32,
    pub offset: u32,
    pub body_len: usize,
}

/// Decode `stream_id(8) + flags(4) + offset(4)`; payload follows the header.
pub fn parse_header(bytes: &[u8]) -> Option<(DataHeader, &[u8])> {
    if bytes.len() < HEADER_BYTES {
        return None;
    }
    let stream_id = u64::from_be_bytes(bytes[0..8].try_into().ok()?);
    let flags = u32::from_be_bytes(bytes[8..12].try_into().ok()?);
    let offset = u32::from_be_bytes(bytes[12..16].try_into().ok()?);
    let body = &bytes[HEADER_BYTES..];
    Some((
        DataHeader {
            stream_id,
            flags,
            offset,
            body_len: body.len(),
        },
        body,
    ))
}

/// Encode one outbound chunk (used by the local client and by tests).
pub fn encode_chunk(stream_id: u64, flags: u32, offset: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_BYTES + payload.len());
    out.extend_from_slice(&stream_id.to_be_bytes());
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

#[derive(Debug)]
struct OpenStream {
    command_id: String,
    declared_size: Option<u64>,
    mime: Option<String>,
    buffer: Vec<u8>,
    idle_since: f64,
}

#[derive(Debug)]
struct StoredBlob {
    command_id: String,
    bytes: Arc<Vec<u8>>,
    mime: Option<String>,
    expires_at: f64,
}

#[derive(Debug, Default)]
struct StoreState {
    streams: HashMap<u64, OpenStream>,
    blobs: HashMap<u64, StoredBlob>,
    cache_bytes: usize,
}

#[derive(Debug, Default)]
pub struct BlobStore {
    state: Mutex<StoreState>,
    next_stream: AtomicU64,
}

/// Outcome of one accepted chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkOutcome {
    Partial { total: usize },
    Complete { stream_id: u64, size: usize },
}

impl BlobStore {
    /// Allocate a stream id for one command's outbound file transfer.
    pub fn new_stream_id(&self) -> u64 {
        self.next_stream.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn open(
        &self,
        stream_id: u64,
        command_id: &str,
        declared_size: Option<u64>,
        mime: Option<&str>,
    ) -> Result<(), StreamError> {
        let mut state = self.lock();
        if state.streams.len() >= MAX_ACTIVE_STREAMS {
            return Err(StreamError::TooManyStreams);
        }
        if let Some(size) = declared_size {
            if size as usize > MAX_STREAM_BYTES {
                return Err(StreamError::TooLarge);
            }
        }
        state.streams.insert(
            stream_id,
            OpenStream {
                command_id: command_id.to_string(),
                declared_size,
                mime: mime.map(str::to_string),
                buffer: Vec::new(),
                idle_since: now_unix_seconds(),
            },
        );
        Ok(())
    }

    /// Append one chunk. Chunks must arrive in order; a gap fails the stream so
    /// the consumer never assembles a corrupt file.
    pub fn push(&self, header: &DataHeader, payload: &[u8]) -> Result<ChunkOutcome, StreamError> {
        let mut state = self.lock();
        let last = (header.flags & wunder_core::interlink::DATA_FLAG_LAST) != 0;
        let aborted = (header.flags & wunder_core::interlink::DATA_FLAG_ERROR) != 0;
        if aborted {
            state.streams.remove(&header.stream_id);
            return Err(StreamError::UnknownStream);
        }
        let offset = header.offset as usize;
        if state
            .streams
            .get(&header.stream_id)
            .map(|stream| stream.buffer.len())
            .is_none()
        {
            return Err(StreamError::UnknownStream);
        }
        let total_after = {
            let stream = state.streams.get_mut(&header.stream_id).expect("checked");
            if stream.buffer.len() != offset {
                return Err(StreamError::OutOfOrder);
            }
            if stream.buffer.len() + payload.len() > MAX_STREAM_BYTES {
                state.streams.remove(&header.stream_id);
                return Err(StreamError::TooLarge);
            }
            stream.buffer.extend_from_slice(payload);
            stream.idle_since = now_unix_seconds();
            stream.buffer.len()
        };
        if !last {
            return Ok(ChunkOutcome::Partial { total: total_after });
        }
        let stream = state
            .streams
            .remove(&header.stream_id)
            .ok_or(StreamError::UnknownStream)?;
        let size = stream.buffer.len();
        if state.cache_bytes + size > MAX_CACHE_BYTES {
            return Err(StreamError::CacheFull);
        }
        state.cache_bytes += size;
        state.blobs.insert(
            header.stream_id,
            StoredBlob {
                command_id: stream.command_id,
                bytes: Arc::new(stream.buffer),
                mime: stream.mime,
                expires_at: now_unix_seconds() + BLOB_TTL_S,
            },
        );
        Ok(ChunkOutcome::Complete {
            stream_id: header.stream_id,
            size,
        })
    }

    /// Fetch an assembled blob by command id (the blob endpoint's lookup).
    pub fn get(&self, command_id: &str) -> Option<Arc<Vec<u8>>> {
        let now = now_unix_seconds();
        let state = self.lock();
        state
            .blobs
            .values()
            .find(|blob| blob.command_id == command_id && blob.expires_at > now)
            .map(|blob| blob.bytes.clone())
    }

    pub fn mime_of(&self, command_id: &str) -> Option<String> {
        let now = now_unix_seconds();
        let state = self.lock();
        state
            .blobs
            .values()
            .find(|blob| blob.command_id == command_id && blob.expires_at > now)
            .and_then(|blob| blob.mime.clone())
    }

    /// Drop expired blobs and streams that stopped transferring; returns the
    /// number of released blobs.
    pub fn cleanup_expired(&self, now: f64) -> usize {
        let mut state = self.lock();
        let expired: Vec<u64> = state
            .blobs
            .iter()
            .filter(|(_, blob)| blob.expires_at <= now)
            .map(|(stream_id, _)| *stream_id)
            .collect();
        let mut freed = 0usize;
        for stream_id in &expired {
            if let Some(blob) = state.blobs.remove(stream_id) {
                freed += 1;
                state.cache_bytes = state.cache_bytes.saturating_sub(blob.bytes.len());
            }
        }
        // A stream whose producer died mid-transfer would otherwise hold one of
        // the 16 slots forever.
        state
            .streams
            .retain(|_, stream| now - stream.idle_since <= BLOB_TTL_S);
        freed
    }

    pub fn cache_bytes(&self) -> usize {
        self.lock().cache_bytes
    }

    pub fn open_streams(&self) -> usize {
        self.lock().streams.len()
    }

    /// Store a small result that arrived inline in `command_result` (docs §6.4:
    /// results up to 1 MiB ride in the frame instead of the data plane) so the
    /// same download endpoint serves both transfer shapes.
    pub fn store_inline(
        &self,
        command_id: &str,
        bytes: Vec<u8>,
        mime: Option<&str>,
    ) -> Result<(), StreamError> {
        if bytes.len() > MAX_STREAM_BYTES {
            return Err(StreamError::TooLarge);
        }
        let mut state = self.lock();
        if state.cache_bytes + bytes.len() > MAX_CACHE_BYTES {
            return Err(StreamError::CacheFull);
        }
        let stream_id = self.next_stream.fetch_add(1, Ordering::Relaxed) + 1;
        state.cache_bytes += bytes.len();
        state.blobs.insert(
            stream_id,
            StoredBlob {
                command_id: command_id.to_string(),
                mime: mime.map(str::to_string),
                bytes: Arc::new(bytes),
                expires_at: now_unix_seconds() + BLOB_TTL_S,
            },
        );
        Ok(())
    }

    /// Read the assembled buffer of one live or finished stream.
    pub fn get_by_stream(&self, stream_id: u64) -> Option<Arc<Vec<u8>>> {
        let state = self.lock();
        if let Some(blob) = state.blobs.get(&stream_id) {
            return Some(blob.bytes.clone());
        }
        state
            .streams
            .get(&stream_id)
            .map(|stream| Arc::new(stream.buffer.clone()))
    }

    /// Forget everything a closed channel left behind (docs §9.3 4).
    pub fn drop_command(&self, command_id: &str) {
        let mut state = self.lock();
        state
            .streams
            .retain(|_, stream| stream.command_id != command_id);
        let removed: Vec<u64> = state
            .blobs
            .iter()
            .filter(|(_, blob)| blob.command_id == command_id)
            .map(|(stream_id, _)| *stream_id)
            .collect();
        for stream_id in removed {
            if let Some(blob) = state.blobs.remove(&stream_id) {
                state.cache_bytes = state.cache_bytes.saturating_sub(blob.bytes.len());
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StoreState> {
        self.state.lock().expect("blob store lock poisoned")
    }
}

pub fn store() -> &'static BlobStore {
    static INSTANCE: OnceLock<BlobStore> = OnceLock::new();
    INSTANCE.get_or_init(BlobStore::default)
}

fn now_unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wunder_core::interlink::{DATA_FLAG_ERROR, DATA_FLAG_LAST};

    fn header(stream_id: u64, flags: u32, offset: u32, payload: &[u8]) -> DataHeader {
        DataHeader {
            stream_id,
            flags,
            offset,
            body_len: payload.len(),
        }
    }

    #[test]
    fn chunk_round_trip_reassembles_the_file() {
        let store = BlobStore::default();
        let bytes = encode_chunk(7, 0, 0, b"hello ");
        let (parsed, body) = parse_header(&bytes).expect("header");
        assert_eq!(parsed.stream_id, 7);
        assert_eq!(parsed.offset, 0);
        assert_eq!(body, b"hello ");

        store.open(7, "cmd_a", Some(12), Some("text/markdown")).expect("open");
        assert_eq!(
            store.push(&parsed, body).expect("chunk"),
            ChunkOutcome::Partial { total: 6 }
        );
        let tail = b"world!";
        let done = store
            .push(&header(7, DATA_FLAG_LAST, 6, tail), tail)
            .expect("final");
        assert_eq!(done, ChunkOutcome::Complete { stream_id: 7, size: 12 });
        assert_eq!(store.get("cmd_a").map(|bytes| bytes.len()), Some(12));
        let assembled = store.get("cmd_a").expect("blob");
        assert_eq!(&assembled[..6], b"hello ");
        assert_eq!(assembled.len(), 12);
        assert_eq!(store.mime_of("cmd_a").as_deref(), Some("text/markdown"));
    }

    #[test]
    fn out_of_order_and_unknown_streams_are_rejected() {
        let store = BlobStore::default();
        assert_eq!(
            store.push(&header(1, 0, 0, b"x"), b"x"),
            Err(StreamError::UnknownStream)
        );
        store.open(2, "cmd_b", None, None).expect("open");
        assert_eq!(
            store.push(&header(2, 0, 5, b"x"), b"x"),
            Err(StreamError::OutOfOrder)
        );
        // The stream survives a bad chunk and can still be fed correctly.
        assert!(store.push(&header(2, 0, 0, b"ok"), b"ok").is_ok());
    }

    #[test]
    fn abort_flag_and_size_limits_fail_closed() {
        let store = BlobStore::default();
        store.open(3, "cmd_c", None, None).expect("open");
        assert_eq!(
            store.push(&header(3, DATA_FLAG_ERROR, 0, b"x"), b"x"),
            Err(StreamError::UnknownStream)
        );
        assert_eq!(store.open_streams(), 0);

        assert_eq!(
            store.open(4, "cmd_d", Some(MAX_STREAM_BYTES as u64 + 1), None),
            Err(StreamError::TooLarge)
        );

        for stream_id in 10..(10 + MAX_ACTIVE_STREAMS as u64) {
            store
                .open(stream_id, "cmd_e", Some(1), None)
                .expect("fill streams");
        }
        assert_eq!(
            store.open(999, "cmd_f", Some(1), None),
            Err(StreamError::TooManyStreams)
        );
    }

    #[test]
    fn cache_is_bounded_and_expires() {
        let store = BlobStore::default();
        store.open(5, "cmd_g", None, None).expect("open");
        let payload = vec![b'z'; 4096];
        store
            .push(&header(5, DATA_FLAG_LAST, 0, &payload), &payload)
            .expect("complete");
        assert_eq!(store.cache_bytes(), 4096);
        assert!(store.get("cmd_g").is_some());
        assert_eq!(store.cleanup_expired(now_unix_seconds() + BLOB_TTL_S + 1.0), 1);
        assert!(store.get("cmd_g").is_none());
        assert_eq!(store.cache_bytes(), 0);

        store.open(6, "cmd_h", None, None).expect("open");
        store.push(&header(6, 0, 0, b"partial"), b"partial").expect("partial");
        store.drop_command("cmd_h");
        assert_eq!(store.open_streams(), 0);
    }

    #[test]
    fn new_stream_ids_are_unique_and_monotonic() {
        let store = BlobStore::default();
        let first = store.new_stream_id();
        let second = store.new_stream_id();
        assert!(second > first);
    }
}
