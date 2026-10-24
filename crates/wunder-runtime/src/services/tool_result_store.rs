//! Private, bounded tool-result carriers. All entry points perform synchronous
//! filesystem work and must run on the caller's filesystem blocking worker.
//!
//! One kernel-owned lock serializes quota changes across processes. A carrier is
//! an immutable UTF-8 JSON payload behind a fixed private header; its opaque
//! handle never contains an owner or a physical path.
use crate::config::Config;
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const HANDLE_PREFIX: &str = "tool-result:";
const MAGIC: &[u8; 16] = b"WUNDERRESULT0001";
const HEADER_BYTES: usize = 128;
const TTL_SECONDS: i64 = 24 * 60 * 60;
const MAX_RESULT_BYTES: u64 = 8 * 1024 * 1024;
const THREAD_BYTES: u64 = 32 * 1024 * 1024;
const THREAD_ITEMS: usize = 128;
const GLOBAL_BYTES: u64 = 256 * 1024 * 1024;
const GLOBAL_ITEMS: usize = 1024;
const MAX_DIRECTORY_ENTRIES: usize = GLOBAL_ITEMS * 4;
const MAX_READ_BYTES: usize = 16 * 1024;
const SEARCH_SCAN_BYTES: usize = 256 * 1024;
const SEARCH_MATCHES: usize = 32;
const SEARCH_QUERY_BYTES: usize = 1024;
const SEARCH_EXCERPT_BYTES: usize = 160;
const LOCK_WAIT: Duration = Duration::from_millis(100);
const LOCK_POLL: Duration = Duration::from_millis(5);

#[derive(Debug)]
pub(crate) struct StoredResult {
    pub handle: String,
    /// Payload bytes, excluding the private header.
    pub bytes: u64,
    pub expires_at: i64,
}

pub(crate) fn store_root(config: &Config) -> PathBuf {
    let root = config.storage.data_root.trim();
    if root.is_empty() {
        PathBuf::from("config/data/tool-results")
    } else {
        PathBuf::from(root).join("tool-results")
    }
}

/// Save complete serialized JSON or return an error. Nothing advertises a
/// partial carrier; both committed files and the active staging file count
/// against byte quotas while serialization is in progress.
pub(crate) fn save(
    root: &Path,
    user_id: &str,
    session_id: &str,
    workspace_id: &str,
    value: &impl Serialize,
) -> Result<StoredResult> {
    let _lock = acquire(root)?;
    let now = now_seconds();
    let owner = owner_key(user_id, session_id, workspace_id);
    let mut inventory = Inventory::load(root, owner, now)?;
    inventory.make_room(HEADER_BYTES as u64)?;

    let id = Uuid::new_v4().simple().to_string();
    let final_path = root.join(format!("{id}.result"));
    let mut temporary = TemporaryFile::create(root.join(format!("{id}.part")))?;
    temporary
        .file
        .as_mut()
        .unwrap()
        .write_all(&[0; HEADER_BYTES])
        .map_err(|error| io_failure("initialize result", error))?;
    let bytes = {
        let mut writer = BufWriter::with_capacity(
            64 * 1024,
            QuotaWriter {
                file: temporary.file.as_mut().unwrap(),
                inventory: &mut inventory,
                payload_bytes: 0,
            },
        );
        serde_json::to_writer(&mut writer, value)
            .map_err(|error| anyhow!("tool result serialization failed: {error}"))?;
        writer
            .flush()
            .map_err(|error| io_failure("write complete result", error))?;
        writer.get_ref().payload_bytes
    };
    let header = Header {
        owner,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
            .min(i64::MAX as u128) as i64,
        expires_at: now.saturating_add(TTL_SECONDS),
        bytes,
    };
    temporary
        .file
        .as_mut()
        .unwrap()
        .seek(SeekFrom::Start(0))
        .and_then(|_| temporary.file.as_mut().unwrap().write_all(&header.encode()))
        .and_then(|_| temporary.file.as_mut().unwrap().flush())
        .map_err(|error| io_failure("finalize result", error))?;
    // Windows requires closing the writer before renaming it.
    let path = temporary.path.clone();
    drop(temporary.file.take());
    fs::rename(&path, &final_path).map_err(|error| io_failure("publish result", error))?;
    temporary.committed = true;
    Ok(StoredResult {
        handle: format!("{HANDLE_PREFIX}{id}"),
        bytes,
        expires_at: header.expires_at,
    })
}

/// Read a bounded UTF-8 page or search literal text in a bounded scan window.
/// Offsets address serialized payload bytes, never characters or header bytes.
pub(crate) fn read(
    root: &Path,
    user_id: &str,
    session_id: &str,
    workspace_id: &str,
    handle: &str,
    offset: u64,
    limit: usize,
    query: Option<&str>,
) -> Result<Value> {
    let id = parse_handle(handle)?;
    let _lock = acquire(root)?;
    let path = root.join(format!("{id}.result"));
    let mut file = open_regular(&path).map_err(|_| unavailable())?;
    let header = Header::read(&mut file).map_err(|_| unavailable())?;
    if header.owner != owner_key(user_id, session_id, workspace_id) {
        return Err(unavailable());
    }
    if header.expires_at <= now_seconds() {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(unavailable());
    }
    if offset > header.bytes {
        bail!("tool result offset exceeds payload length");
    }
    file.seek(SeekFrom::Start(HEADER_BYTES as u64 + offset))
        .map_err(|error| io_failure("seek result", error))?;
    match query {
        Some(query) => search_page(file, handle, &header, offset, limit, query),
        None => read_page(file, handle, &header, offset, limit),
    }
}

fn read_page(
    mut file: File,
    handle: &str,
    header: &Header,
    offset: u64,
    limit: usize,
) -> Result<Value> {
    // At least one four-byte UTF-8 scalar must fit, even for a tiny limit.
    let length = (header.bytes - offset).min(limit.clamp(4, MAX_READ_BYTES) as u64) as usize;
    let content = read_utf8(&mut file, length)?;
    let next = offset + content.len() as u64;
    Ok(json!({
        "result_handle": handle,
        "offset": offset,
        "content": content,
        "next_offset": (next < header.bytes).then_some(next),
        "has_more": next < header.bytes,
        "total_bytes": header.bytes,
        "expires_at": header.expires_at,
    }))
}

fn search_page(
    mut file: File,
    handle: &str,
    header: &Header,
    offset: u64,
    limit: usize,
    query: &str,
) -> Result<Value> {
    if query.is_empty() || query.len() > SEARCH_QUERY_BYTES {
        bail!("tool result query must contain 1 to {SEARCH_QUERY_BYTES} UTF-8 bytes");
    }
    let length = (header.bytes - offset)
        .min((SEARCH_SCAN_BYTES + query.len() - 1 + SEARCH_EXCERPT_BYTES) as u64)
        as usize;
    let text = read_utf8(&mut file, length)?;
    let scan_end = floor_boundary(&text, SEARCH_SCAN_BYTES.min(text.len()));
    let mut next = scan_end;
    let mut matches = Vec::with_capacity(SEARCH_MATCHES);
    let mut content_remaining = limit.clamp(4, MAX_READ_BYTES);
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find(query) {
        let index = search_from + relative;
        if index >= scan_end {
            break;
        }
        if matches.len() == SEARCH_MATCHES || content_remaining < 4 {
            next = index;
            break;
        }
        let end = floor_boundary(
            &text,
            (index + SEARCH_EXCERPT_BYTES.min(content_remaining)).min(text.len()),
        );
        matches.push(json!({"offset": offset + index as u64, "content": &text[index..end]}));
        content_remaining -= end - index;
        // Include overlapping literal matches. One-scalar advancement preserves
        // UTF-8 slicing and consistent semantics across continuation pages.
        search_from = index + text[index..].chars().next().unwrap().len_utf8();
    }
    let next = offset + next as u64;
    Ok(json!({
        "result_handle": handle,
        "offset": offset,
        "matches": matches,
        "scanned_bytes": next - offset,
        "next_offset": (next < header.bytes).then_some(next),
        "has_more": next < header.bytes,
        "total_bytes": header.bytes,
        "expires_at": header.expires_at,
    }))
}

fn read_utf8(file: &mut File, length: usize) -> Result<String> {
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)
        .map_err(|error| io_failure("read result", error))?;
    match std::str::from_utf8(&bytes) {
        Ok(_) => {}
        Err(error) if error.error_len().is_none() && error.valid_up_to() > 0 => {
            bytes.truncate(error.valid_up_to());
        }
        Err(_) => bail!("tool result offset must be on a UTF-8 boundary"),
    }
    String::from_utf8(bytes).map_err(|_| anyhow!("invalid UTF-8 tool result"))
}

fn floor_boundary(text: &str, mut end: usize) -> usize {
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn parse_handle(handle: &str) -> Result<&str> {
    let id = handle
        .strip_prefix(HANDLE_PREFIX)
        .filter(|id| valid_id(id))
        .ok_or_else(|| anyhow!("invalid tool result handle"))?;
    Ok(id)
}

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn owner_key(user_id: &str, session_id: &str, workspace_id: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in [user_id, session_id, workspace_id] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    digest.finalize().into()
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn unavailable() -> anyhow::Error {
    anyhow!("tool result is unavailable or expired")
}

fn io_failure(operation: &str, error: io::Error) -> anyhow::Error {
    // Filesystem errors can embed physical paths. Expose the operation and kind.
    anyhow!("{operation} failed ({:?})", error.kind())
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn acquire(root: &Path) -> Result<File> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(root)
        .map_err(|error| io_failure("prepare result store", error))?;
    let lock = private_options()
        .create(true)
        .truncate(false)
        .open(root.join(".lock"))
        .map_err(|error| io_failure("open result store lock", error))?;
    let started = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(TryLockError::WouldBlock) if started.elapsed() < LOCK_WAIT => {
                std::thread::sleep(LOCK_POLL.min(LOCK_WAIT.saturating_sub(started.elapsed())));
            }
            Err(TryLockError::WouldBlock) => bail!("tool result store is busy; retry later"),
            Err(TryLockError::Error(error)) => return Err(io_failure("lock result store", error)),
        }
    }
}

fn open_regular(path: &Path) -> io::Result<File> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular result",
        ));
    }
    File::open(path)
}

#[derive(Clone)]
struct Header {
    owner: [u8; 32],
    created_at: i64,
    expires_at: i64,
    bytes: u64,
}

impl Header {
    fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut bytes = [0; HEADER_BYTES];
        bytes[..16].copy_from_slice(MAGIC);
        bytes[16..24].copy_from_slice(&self.created_at.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.expires_at.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.bytes.to_le_bytes());
        bytes[40..72].copy_from_slice(&self.owner);
        bytes
    }

    fn read(file: &mut File) -> io::Result<Self> {
        let mut data = [0; HEADER_BYTES];
        file.read_exact(&mut data)?;
        let header = Self {
            created_at: i64::from_le_bytes(data[16..24].try_into().unwrap()),
            expires_at: i64::from_le_bytes(data[24..32].try_into().unwrap()),
            bytes: u64::from_le_bytes(data[32..40].try_into().unwrap()),
            owner: data[40..72].try_into().unwrap(),
        };
        if &data[..16] != MAGIC
            || header.bytes > MAX_RESULT_BYTES
            || file.metadata()?.len() != header.bytes + HEADER_BYTES as u64
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid result header",
            ));
        }
        Ok(header)
    }
}

struct Entry {
    path: PathBuf,
    header: Header,
}

struct Inventory {
    entries: Vec<Option<Entry>>,
    owner: [u8; 32],
    global_bytes: u64,
    global_count: usize,
    owner_bytes: u64,
    owner_count: usize,
}

impl Inventory {
    fn load(root: &Path, owner: [u8; 32], now: i64) -> Result<Self> {
        let mut entries = Vec::with_capacity(GLOBAL_ITEMS);
        let directory =
            fs::read_dir(root).map_err(|error| io_failure("scan result store", error))?;
        for (index, entry) in directory.enumerate() {
            if index >= MAX_DIRECTORY_ENTRIES {
                bail!("tool result store directory limit exceeded");
            }
            let entry = entry.map_err(|error| io_failure("inspect result store", error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((id, extension)) = name.rsplit_once('.') else {
                continue;
            };
            if !valid_id(id) || !matches!(extension, "part" | "result") {
                continue;
            }
            let path = entry.path();
            if extension == "part" {
                fs::remove_file(&path)
                    .map_err(|error| io_failure("clean incomplete result", error))?;
                continue;
            }
            let header = open_regular(&path).and_then(|mut file| Header::read(&mut file));
            match header {
                Ok(header) if header.expires_at > now => entries.push(Some(Entry { path, header })),
                _ => {
                    fs::remove_file(&path)
                        .map_err(|error| io_failure("clean expired result", error))?;
                }
            }
        }
        entries.sort_by_key(|entry| entry.as_ref().map(|entry| entry.header.created_at));
        let mut inventory = Self {
            entries,
            owner,
            global_bytes: 0,
            global_count: 0,
            owner_bytes: 0,
            owner_count: 0,
        };
        for entry in inventory.entries.iter().flatten() {
            let bytes = entry.header.bytes + HEADER_BYTES as u64;
            inventory.global_bytes += bytes;
            inventory.global_count += 1;
            if entry.header.owner == owner {
                inventory.owner_bytes += bytes;
                inventory.owner_count += 1;
            }
        }
        Ok(inventory)
    }

    fn make_room(&mut self, staged_bytes: u64) -> Result<()> {
        while self.owner_count + 1 > THREAD_ITEMS || self.owner_bytes + staged_bytes > THREAD_BYTES
        {
            let index = self
                .entries
                .iter()
                .position(|entry| {
                    entry
                        .as_ref()
                        .is_some_and(|entry| entry.header.owner == self.owner)
                })
                .ok_or_else(|| anyhow!("tool result exceeds thread storage quota"))?;
            self.evict(index)?;
        }
        while self.global_count + 1 > GLOBAL_ITEMS
            || self.global_bytes + staged_bytes > GLOBAL_BYTES
        {
            let index = self
                .entries
                .iter()
                .position(Option::is_some)
                .ok_or_else(|| anyhow!("tool result exceeds global storage quota"))?;
            self.evict(index)?;
        }
        Ok(())
    }

    fn evict(&mut self, index: usize) -> Result<()> {
        let entry = self.entries[index].as_ref().unwrap();
        fs::remove_file(&entry.path).map_err(|error| io_failure("evict old result", error))?;
        let entry = self.entries[index].take().unwrap();
        let bytes = entry.header.bytes + HEADER_BYTES as u64;
        self.global_bytes -= bytes;
        self.global_count -= 1;
        if entry.header.owner == self.owner {
            self.owner_bytes -= bytes;
            self.owner_count -= 1;
        }
        Ok(())
    }
}

struct QuotaWriter<'a> {
    file: &'a mut File,
    inventory: &'a mut Inventory,
    payload_bytes: u64,
}

impl Write for QuotaWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self.payload_bytes.saturating_add(bytes.len() as u64);
        if next > MAX_RESULT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tool result exceeds 8 MiB carrier limit",
            ));
        }
        self.inventory
            .make_room(next + HEADER_BYTES as u64)
            .map_err(|error| io::Error::other(error.to_string()))?;
        self.file.write_all(bytes)?;
        self.payload_bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

struct TemporaryFile {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl TemporaryFile {
    fn create(path: PathBuf) -> Result<Self> {
        let file = private_options()
            .create_new(true)
            .open(&path)
            .map_err(|error| io_failure("create result carrier", error))?;
        Ok(Self {
            path,
            file: Some(file),
            committed: false,
        })
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if !self.committed {
            drop(self.file.take());
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save_fixture(root: &Path, value: &impl Serialize) -> StoredResult {
        save(root, "user-a", "thread-a", "workspace-a", value).unwrap()
    }

    fn read_fixture(root: &Path, handle: &str, offset: u64, limit: usize) -> Result<Value> {
        read(
            root,
            "user-a",
            "thread-a",
            "workspace-a",
            handle,
            offset,
            limit,
            None,
        )
    }

    #[test]
    fn root_uses_private_data_directory_under_storage_root() {
        let mut config = Config::default();
        config.storage.data_root.clear();
        assert_eq!(
            store_root(&config),
            PathBuf::from("config").join("data").join("tool-results")
        );
        config.storage.data_root = "state".into();
        assert_eq!(store_root(&config), PathBuf::from("state/tool-results"));
    }

    #[test]
    fn unicode_pages_reconstruct_complete_json_with_small_limits() {
        let root = tempfile::tempdir().unwrap();
        let value = json!({"content": "示例🧪文字".repeat(100)});
        let stored = save_fixture(root.path(), &value);
        let mut offset = 0;
        let mut restored = String::new();
        loop {
            let page = read_fixture(root.path(), &stored.handle, offset, 7).unwrap();
            restored.push_str(page["content"].as_str().unwrap());
            let Some(next) = page["next_offset"].as_u64() else {
                break;
            };
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(restored, serde_json::to_string(&value).unwrap());
        assert_eq!(stored.bytes, restored.len() as u64);
        assert!(stored.expires_at > now_seconds());
        let page = read_fixture(root.path(), &stored.handle, 0, usize::MAX).unwrap();
        assert!(page["content"].as_str().unwrap().len() <= MAX_READ_BYTES);
    }

    #[test]
    fn handles_enforce_all_three_owner_dimensions_and_reject_paths() {
        let root = tempfile::tempdir().unwrap();
        let stored = save_fixture(root.path(), &json!({"ok": true}));
        for (user, session, workspace) in [
            ("user-b", "thread-a", "workspace-a"),
            ("user-a", "thread-b", "workspace-a"),
            ("user-a", "thread-a", "workspace-b"),
        ] {
            assert!(read(
                root.path(),
                user,
                session,
                workspace,
                &stored.handle,
                0,
                100,
                None
            )
            .is_err());
        }
        for handle in [
            "../example",
            "tool-result:../example",
            "tool-result:ABCDEF",
            "tool-result:",
        ] {
            assert!(read_fixture(root.path(), handle, 0, 100).is_err());
        }
        assert_ne!(owner_key("ab", "c", "d"), owner_key("a", "bc", "d"));
    }

    #[test]
    fn offsets_are_utf8_byte_boundaries_and_payload_bounds_are_checked() {
        let root = tempfile::tempdir().unwrap();
        let value = json!({"text": "示例🧪"});
        let stored = save_fixture(root.path(), &value);
        let serialized = serde_json::to_string(&value).unwrap();
        let start = serialized.find('示').unwrap() as u64;
        assert!(read_fixture(root.path(), &stored.handle, start + 1, 100)
            .unwrap_err()
            .to_string()
            .contains("UTF-8 boundary"));
        assert!(read_fixture(root.path(), &stored.handle, stored.bytes + 1, 100).is_err());
        let end = read_fixture(root.path(), &stored.handle, stored.bytes, 100).unwrap();
        assert_eq!(end["content"], "");
        assert_eq!(end["has_more"], false);
    }

    #[test]
    fn literal_search_is_bounded_and_continuation_finds_every_match() {
        let root = tempfile::tempdir().unwrap();
        let value = json!({"text": format!("{}{}", "x".repeat(SEARCH_SCAN_BYTES - 6), "示例[needle]".repeat(100))});
        let stored = save_fixture(root.path(), &value);
        let serialized = serde_json::to_string(&value).unwrap();
        let expected: Vec<u64> = serialized
            .match_indices("[needle]")
            .map(|(index, _)| index as u64)
            .collect();
        let mut found = Vec::new();
        let mut offset = 0;
        loop {
            let page = read(
                root.path(),
                "user-a",
                "thread-a",
                "workspace-a",
                &stored.handle,
                offset,
                100,
                Some("[needle]"),
            )
            .unwrap();
            let matches = page["matches"].as_array().unwrap();
            assert!(matches.len() <= SEARCH_MATCHES);
            found.extend(matches.iter().map(|item| item["offset"].as_u64().unwrap()));
            assert!(page["scanned_bytes"].as_u64().unwrap() <= SEARCH_SCAN_BYTES as u64);
            let Some(next) = page["next_offset"].as_u64() else {
                break;
            };
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(found, expected);
    }

    #[test]
    fn literal_search_preserves_overlapping_matches_across_scan_boundary() {
        let root = tempfile::tempdir().unwrap();
        let value =
            json!({"text": format!("{}{}", "x".repeat(SEARCH_SCAN_BYTES - 14), "a".repeat(45))});
        let stored = save_fixture(root.path(), &value);
        let serialized = serde_json::to_string(&value).unwrap();
        let expected: Vec<u64> = serialized
            .as_bytes()
            .windows(3)
            .enumerate()
            .filter_map(|(index, part)| (part == b"aaa").then_some(index as u64))
            .collect();
        let mut found = Vec::new();
        let mut offset = 0;
        loop {
            let page = read(
                root.path(),
                "user-a",
                "thread-a",
                "workspace-a",
                &stored.handle,
                offset,
                100,
                Some("aaa"),
            )
            .unwrap();
            found.extend(
                page["matches"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|item| item["offset"].as_u64().unwrap()),
            );
            let Some(next) = page["next_offset"].as_u64() else {
                break;
            };
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(found, expected);
    }

    #[test]
    fn literal_search_finds_multibyte_query_straddling_scan_boundary() {
        let root = tempfile::tempdir().unwrap();
        let value = json!({"text": format!("{}示例needle🧪", "x".repeat(SEARCH_SCAN_BYTES - 13))});
        let stored = save_fixture(root.path(), &value);
        let serialized = serde_json::to_string(&value).unwrap();
        let query = "示例needle🧪";
        let page = read(
            root.path(),
            "user-a",
            "thread-a",
            "workspace-a",
            &stored.handle,
            0,
            100,
            Some(query),
        )
        .unwrap();
        assert_eq!(
            page["matches"][0]["offset"],
            serialized.find(query).unwrap() as u64
        );
        assert_eq!(page["scanned_bytes"], (SEARCH_SCAN_BYTES - 1) as u64);
        let next = page["next_offset"].as_u64().unwrap();
        let continuation = read(
            root.path(),
            "user-a",
            "thread-a",
            "workspace-a",
            &stored.handle,
            next,
            100,
            Some(query),
        )
        .unwrap();
        assert_eq!(continuation["matches"], json!([]));
    }

    #[test]
    fn literal_search_limit_bounds_content_and_changed_limits_preserve_all_matches() {
        let root = tempfile::tempdir().unwrap();
        let value = json!({"text": "示例🧪match tail ".repeat(90)});
        let stored = save_fixture(root.path(), &value);
        let serialized = serde_json::to_string(&value).unwrap();
        let expected: Vec<u64> = serialized
            .match_indices("示例")
            .map(|(index, _)| index as u64)
            .collect();
        let search = |offset, limit| {
            read(
                root.path(),
                "user-a",
                "thread-a",
                "workspace-a",
                &stored.handle,
                offset,
                limit,
                Some("示例"),
            )
            .unwrap()
        };
        let small = search(0, 4);
        let large = search(0, 1024);
        assert!(
            small["matches"][0]["content"].as_str().unwrap().len()
                < large["matches"][0]["content"].as_str().unwrap().len()
        );
        assert!(small["next_offset"].as_u64().unwrap() < large["next_offset"].as_u64().unwrap());
        let limits = [4, 32, 512, 0, usize::MAX];
        let mut offset = 0;
        let mut page_index = 0;
        let mut found = Vec::new();
        loop {
            let limit = limits[page_index % limits.len()];
            let page = search(offset, limit);
            let matches = page["matches"].as_array().unwrap();
            let content_bytes: usize = matches
                .iter()
                .map(|item| item["content"].as_str().unwrap().len())
                .sum();
            assert!(content_bytes <= limit.clamp(4, MAX_READ_BYTES));
            found.extend(matches.iter().map(|item| item["offset"].as_u64().unwrap()));
            let Some(next) = page["next_offset"].as_u64() else {
                break;
            };
            assert!(next > offset);
            offset = next;
            page_index += 1;
        }
        assert_eq!(found, expected);
    }

    #[test]
    fn expiry_removes_carrier_and_cleanup_discards_orphan_staging_file() {
        let root = tempfile::tempdir().unwrap();
        let stored = save_fixture(root.path(), &json!({"content": "sample"}));
        let path = root
            .path()
            .join(format!("{}.result", parse_handle(&stored.handle).unwrap()));
        let mut file = private_options().open(&path).unwrap();
        let mut header = Header::read(&mut file).unwrap();
        header.expires_at = now_seconds() - 1;
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&header.encode()).unwrap();
        drop(file);
        assert!(read_fixture(root.path(), &stored.handle, 0, 100).is_err());
        assert!(!path.exists());
        let orphan = root
            .path()
            .join(format!("{}.part", Uuid::new_v4().simple()));
        fs::write(&orphan, b"partial").unwrap();
        save_fixture(root.path(), &json!({"ok": true}));
        assert!(!orphan.exists());
    }

    #[test]
    fn oversized_or_failed_serialization_publishes_no_partial_carrier() {
        struct Broken;
        impl Serialize for Broken {
            fn serialize<S: serde::Serializer>(
                &self,
                _: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("fixture serialization failure"))
            }
        }
        let root = tempfile::tempdir().unwrap();
        assert!(save(
            root.path(),
            "u",
            "s",
            "w",
            &"x".repeat(MAX_RESULT_BYTES as usize)
        )
        .is_err());
        assert!(save(root.path(), "u", "s", "w", &Broken).is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1); // persistent lock only
        let blocked = root.path().join("unavailable");
        fs::write(&blocked, b"file instead of directory").unwrap();
        let error = save(&blocked, "u", "s", "w", &json!({}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("prepare result store failed"));
        assert!(!error.contains(&blocked.display().to_string()));
    }

    #[test]
    fn kernel_lock_waits_only_for_bounded_interval() {
        let root = tempfile::tempdir().unwrap();
        let lock = acquire(root.path()).unwrap();
        let started = Instant::now();
        assert!(save(root.path(), "u", "s", "w", &json!({}))
            .unwrap_err()
            .to_string()
            .contains("busy"));
        assert!(started.elapsed() >= LOCK_WAIT);
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(lock);
        save_fixture(root.path(), &json!({"ok": true}));
    }

    #[test]
    fn thread_item_quota_evicts_oldest_and_preserves_other_threads() {
        let root = tempfile::tempdir().unwrap();
        let other = save(
            root.path(),
            "user-a",
            "thread-b",
            "workspace-a",
            &json!({"other": true}),
        )
        .unwrap();
        let oldest = save_fixture(root.path(), &json!({"first": true}));
        // Make order deterministic without sleeps or timestamp assumptions.
        let path = root
            .path()
            .join(format!("{}.result", parse_handle(&oldest.handle).unwrap()));
        let mut file = private_options().open(path).unwrap();
        let mut header = Header::read(&mut file).unwrap();
        header.created_at -= 1;
        file.seek(SeekFrom::Start(0)).unwrap();
        file.write_all(&header.encode()).unwrap();
        drop(file);
        for index in 0..THREAD_ITEMS {
            save_fixture(root.path(), &json!({"index": index}));
        }
        assert!(read_fixture(root.path(), &oldest.handle, 0, 100).is_err());
        assert!(read(
            root.path(),
            "user-a",
            "thread-b",
            "workspace-a",
            &other.handle,
            0,
            100,
            None
        )
        .is_ok());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), THREAD_ITEMS + 2);
    }

    #[test]
    fn byte_quotas_include_staging_and_global_eviction_is_bounded() {
        let root = tempfile::tempdir().unwrap();
        let mut entries = Vec::new();
        for index in 0..3 {
            let path = root.path().join(format!("{index:032x}.result"));
            fs::write(&path, b"quota fixture").unwrap();
            entries.push(Some(Entry {
                path,
                header: Header {
                    owner: if index == 2 { [2; 32] } else { [1; 32] },
                    created_at: index,
                    expires_at: now_seconds() + TTL_SECONDS,
                    bytes: THREAD_BYTES / 2 - HEADER_BYTES as u64,
                },
            }));
        }
        let mut inventory = Inventory {
            entries,
            owner: [1; 32],
            global_bytes: THREAD_BYTES * 3 / 2,
            global_count: 3,
            owner_bytes: THREAD_BYTES,
            owner_count: 2,
        };
        inventory.make_room(HEADER_BYTES as u64 + 1).unwrap();
        assert!(inventory.entries[0].is_none());
        assert!(inventory.entries[1].is_some() && inventory.entries[2].is_some());
        inventory.global_bytes = GLOBAL_BYTES;
        inventory.make_room(HEADER_BYTES as u64 + 1).unwrap();
        assert!(inventory.entries[1].is_none());
        assert!(inventory.entries[2].is_some());
    }
}
