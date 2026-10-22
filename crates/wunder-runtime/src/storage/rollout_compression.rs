//! Explicit cold-rollout compression. No timer or idle scan is started.
//!
//! Metadata keeps the logical `.jsonl` path. Representation locks coordinate
//! writers and maintenance; readers retain an opened representation snapshot.
use anyhow::{anyhow, bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File, FileTimes, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tempfile::NamedTempFile;

const OPEN_RETRIES: usize = 3;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(50);
const MAINTENANCE_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone)]
pub struct CompressionOptions {
    pub enabled: bool,
    pub level: i32,
    pub min_age_days: u64,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            level: 3,
            min_age_days: 7,
        }
    }
}

pub fn logical_path(path: &Path) -> PathBuf {
    if path.extension().and_then(|v| v.to_str()) == Some("zst") {
        path.with_extension("")
    } else {
        path.to_owned()
    }
}

pub fn compressed_path(path: &Path) -> PathBuf {
    let plain = logical_path(path);
    let mut name = plain.as_os_str().to_os_string();
    name.push(".zst");
    PathBuf::from(name)
}

/// Retain this guard until every writable file handle has been closed.
pub(crate) fn try_representation_lock(path: &Path) -> Result<Option<File>> {
    let plain = logical_path(path);
    let parent = plain
        .parent()
        .ok_or_else(|| anyhow!("rollout has no parent"))?;
    let directory = parent.join(".locks");
    fs::create_dir_all(&directory)?;
    let name = plain
        .file_name()
        .ok_or_else(|| anyhow!("rollout has no filename"))?;
    let digest = Sha256::digest(name.as_encoded_bytes());
    let guard = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(format!("{digest:x}.lock")))?;
    match guard.try_lock() {
        Ok(()) => Ok(Some(guard)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Seekable logical bytes. Compressed input is streamed to an anonymous
/// temporary file with bounded memory. A read never publishes a plain rollout.
pub struct RolloutReader {
    file: File,
    len: u64,
}

impl RolloutReader {
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn into_file(self) -> File {
        self.file
    }
}

impl Read for RolloutReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.file.read(bytes)
    }
}

impl Seek for RolloutReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

fn open_representation(path: &Path) -> io::Result<(File, bool)> {
    let plain = logical_path(path);
    for attempt in 0..=OPEN_RETRIES {
        match File::open(&plain) {
            Ok(file) => return Ok((file, false)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        match File::open(compressed_path(&plain)) {
            Ok(file) => return Ok((file, true)),
            Err(error) if error.kind() == io::ErrorKind::NotFound && attempt < OPEN_RETRIES => {
                std::thread::sleep(OPEN_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

/// Sequential readers can stop at a bounded prefix without temporary decoding.
pub fn open_rollout_stream(path: impl AsRef<Path>) -> Result<Box<dyn Read + Send>> {
    let (file, compressed) = open_representation(path.as_ref())?;
    if compressed {
        Ok(Box::new(zstd::Decoder::new(file)?))
    } else {
        Ok(Box::new(file))
    }
}

pub fn open_rollout_reader(path: impl AsRef<Path>) -> Result<RolloutReader> {
    let (mut file, compressed) = open_representation(path.as_ref())?;
    let len = if compressed {
        let mut decoded = tempfile::tempfile().context("create logical rollout read snapshot")?;
        let mut decoder = zstd::Decoder::new(file)?;
        let len = io::copy(&mut decoder, &mut decoded).context("decode rollout read snapshot")?;
        decoded.seek(SeekFrom::Start(0))?;
        file = decoded;
        len
    } else {
        file.metadata()?.len()
    };
    Ok(RolloutReader { file, len })
}

/// Our compressor emits exactly one pledged-size frame. Reading its bounded
/// header lets an already-current projection avoid decoding a cold file.
pub(crate) fn logical_rollout_len(path: &Path) -> Result<Option<u64>> {
    let (mut file, compressed) = open_representation(path)?;
    if !compressed {
        return Ok(Some(file.metadata()?.len()));
    }
    let mut header = [0u8; 18];
    let count = file.read(&mut header)?;
    let size = zstd::zstd_safe::get_frame_content_size(&header[..count]);
    if size >= u64::MAX - 1 {
        Ok(None)
    } else {
        Ok(Some(size))
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SourceIdentity {
    len: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    readonly: bool,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    mode: u32,
}

impl SourceIdentity {
    fn read(metadata: &Metadata) -> Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified()?,
            created: metadata.created().ok(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            mode: metadata.mode(),
        })
    }
}

struct DigestWriter(Sha256);
impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn file_digest(mut input: impl Read) -> Result<[u8; 32]> {
    let mut digest = DigestWriter(Sha256::new());
    io::copy(&mut input, &mut digest)?;
    Ok(digest.0.finalize().into())
}

fn preserve_attributes(file: &File, metadata: &Metadata) -> Result<()> {
    file.set_permissions(metadata.permissions())?;
    file.set_times(FileTimes::new().set_modified(metadata.modified()?))?;
    file.sync_all()?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<()> {
    // std cannot open Windows directories; file barriers still apply there.
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn compress_rollout(
    path: impl AsRef<Path>,
    options: &CompressionOptions,
) -> Result<Option<PathBuf>> {
    let path = logical_path(path.as_ref());
    if !options.enabled || path.extension().and_then(|v| v.to_str()) != Some("jsonl") {
        return Ok(None);
    }
    let Some(_guard) = try_representation_lock(&path)? else {
        return Ok(None);
    };
    let mut input = match File::open(&path) {
        Ok(input) => input,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = input.metadata()?;
    let identity = SourceIdentity::read(&metadata)?;
    let min_age = Duration::from_secs(options.min_age_days.saturating_mul(24 * 60 * 60));
    if metadata.len() == 0
        || SystemTime::now()
            .duration_since(identity.modified)
            .unwrap_or_default()
            < min_age
    {
        return Ok(None);
    }
    let target = compressed_path(&path);
    // Never replace an existing representation after interrupted publication.
    if target.try_exists()? {
        return Ok(None);
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("rollout has no parent"))?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    let mut encoder = zstd::Encoder::new(temporary.as_file_mut(), options.level.clamp(1, 19))?;
    encoder.set_pledged_src_size(Some(identity.len))?;
    let mut source_digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        source_digest.update(&buffer[..count]);
        encoder.write_all(&buffer[..count])?;
    }
    encoder.finish()?;
    let source_digest: [u8; 32] = source_digest.finalize().into();
    temporary.as_file_mut().seek(SeekFrom::Start(0))?;
    if file_digest(zstd::Decoder::new(temporary.as_file_mut())?)? != source_digest {
        bail!("compressed rollout verification failed");
    }
    preserve_attributes(temporary.as_file(), &metadata)?;
    if SourceIdentity::read(&fs::metadata(&path)?)? != identity
        || file_digest(File::open(&path)?)? != source_digest
    {
        bail!("rollout changed during compression");
    }
    let published = temporary.persist_noclobber(&target).map_err(|e| e.error)?;
    published.sync_all()?;
    drop(published);
    sync_parent(&target)?;
    if SourceIdentity::read(&fs::metadata(&path)?)? != identity {
        bail!("rollout changed before compressed publication completed");
    }
    drop(input); // Release source handles before removing on Windows.
    fs::remove_file(&path)?;
    sync_parent(&path)?;
    Ok(Some(target))
}

/// Caller retains the same representation lock through its writable lifetime.
pub(crate) fn materialize_rollout_locked(path: &Path) -> Result<PathBuf> {
    let plain = logical_path(path);
    if plain.try_exists()? {
        return Ok(plain);
    }
    let compressed = compressed_path(&plain);
    let input = match File::open(&compressed) {
        Ok(input) => input,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(plain),
        Err(error) => return Err(error.into()),
    };
    let metadata = input.metadata()?;
    let identity = SourceIdentity::read(&metadata)?;
    let parent = plain
        .parent()
        .ok_or_else(|| anyhow!("rollout has no parent"))?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    {
        let mut decoder = zstd::Decoder::new(input)?;
        io::copy(&mut decoder, temporary.as_file_mut())
            .context("materialize compressed rollout")?;
    }
    preserve_attributes(temporary.as_file(), &metadata)?;
    if SourceIdentity::read(&fs::metadata(&compressed)?)? != identity {
        bail!("compressed rollout changed during materialization");
    }
    let published = temporary.persist_noclobber(&plain).map_err(|e| e.error)?;
    published.sync_all()?;
    drop(published);
    sync_parent(&plain)?;
    fs::remove_file(compressed)?;
    sync_parent(&plain)?;
    Ok(plain)
}

pub fn materialize_rollout(path: impl AsRef<Path>) -> Result<PathBuf> {
    let plain = logical_path(path.as_ref());
    let _guard = try_representation_lock(&plain)?
        .ok_or_else(|| anyhow!("rollout has an active writer or maintenance owner"))?;
    materialize_rollout_locked(&plain)
}

pub(crate) fn remove_rollout_representations(path: &Path) -> Result<()> {
    let plain = logical_path(path);
    let _guard = try_representation_lock(&plain)?
        .ok_or_else(|| anyhow!("rollout has an active writer or maintenance owner"))?;
    for candidate in [&plain, &compressed_path(&plain)] {
        match fs::remove_file(candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    sync_parent(&plain)
}

pub(crate) fn move_rollout_representations(source: &Path, target: &Path) -> Result<()> {
    let source = logical_path(source);
    let target = logical_path(target);
    let _guard = try_representation_lock(&source)?
        .ok_or_else(|| anyhow!("rollout has an active writer or maintenance owner"))?;
    if target.try_exists()? || compressed_path(&target).try_exists()? {
        bail!("rollout move target already exists");
    }
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("rollout target has no parent"))?;
    fs::create_dir_all(parent)?;
    let plain_exists = source.try_exists()?;
    let compressed_exists = compressed_path(&source).try_exists()?;
    if !plain_exists && !compressed_exists {
        bail!("rollout source is missing");
    }
    if plain_exists {
        fs::rename(&source, &target)?;
    }
    if compressed_exists {
        if let Err(error) = fs::rename(compressed_path(&source), compressed_path(&target)) {
            if plain_exists {
                let _ = fs::rename(&target, &source);
            }
            return Err(error.into());
        }
    }
    sync_parent(&source)?;
    sync_parent(&target)
}

/// Explicit pass marker: lock prevents overlap and success leaves a six-hour
/// cooldown. No timer, recurring task, or polling is associated with it.
pub(crate) struct MaintenancePass {
    file: File,
    completed: bool,
}

impl MaintenancePass {
    pub(crate) fn begin(root: &Path) -> Result<Option<Self>> {
        let directory = root.join(".tmp");
        fs::create_dir_all(&directory)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join("rollout-compression.lock"))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let metadata = file.metadata()?;
        if metadata.len() > 0
            && metadata
                .modified()
                .ok()
                .and_then(|time| SystemTime::now().duration_since(time).ok())
                .is_some_and(|elapsed| elapsed < MAINTENANCE_COOLDOWN)
        {
            return Ok(None);
        }
        Ok(Some(Self {
            file,
            completed: false,
        }))
    }

    pub(crate) fn complete(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(b"complete\n")?;
        self.file.sync_all()?;
        self.completed = true;
        Ok(())
    }
}

impl Drop for MaintenancePass {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.file.set_len(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn enabled() -> CompressionOptions {
        CompressionOptions {
            enabled: true,
            min_age_days: 0,
            ..Default::default()
        }
    }

    #[test]
    fn compressed_read_seek_and_materialize_preserve_logical_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("rollout.jsonl");
        let bytes = "{\"text\":\"示例🙂\"}\n".repeat(12_000).into_bytes();
        fs::write(&path, &bytes).unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let compressed = compress_rollout(&path, &enabled()).unwrap().unwrap();
        assert!(!path.exists());
        assert!(fs::metadata(&compressed).unwrap().len() < bytes.len() as u64 / 10);
        let mut stream = open_rollout_stream(&path).unwrap();
        let mut actual = Vec::new();
        stream.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, bytes);
        drop(stream);
        let mut reader = open_rollout_reader(&path).unwrap();
        assert_eq!(reader.len(), bytes.len() as u64);
        reader.seek(SeekFrom::End(-13)).unwrap();
        let mut tail = Vec::new();
        reader.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, bytes[bytes.len() - 13..]);
        drop(reader);
        assert!(!path.exists(), "read does not publish a plain rollout");
        materialize_rollout(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert!(!compressed.exists());
    }

    #[test]
    fn disabled_young_busy_and_existing_target_preserve_source() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("rollout.jsonl");
        fs::write(&path, b"example\n").unwrap();
        assert!(compress_rollout(&path, &Default::default())
            .unwrap()
            .is_none());
        let young = CompressionOptions {
            min_age_days: 7,
            ..enabled()
        };
        assert!(compress_rollout(&path, &young).unwrap().is_none());
        let guard = try_representation_lock(&path).unwrap().unwrap();
        assert!(compress_rollout(&path, &enabled()).unwrap().is_none());
        drop(guard);
        fs::write(compressed_path(&path), b"existing").unwrap();
        assert!(compress_rollout(&path, &enabled()).unwrap().is_none());
        assert_eq!(fs::read(&path).unwrap(), b"example\n");
        assert_eq!(fs::read(compressed_path(&path)).unwrap(), b"existing");
    }

    #[test]
    fn maintenance_marker_is_locked_and_success_has_cooldown() {
        let root = tempfile::tempdir().unwrap();
        let pass = MaintenancePass::begin(root.path()).unwrap().unwrap();
        assert!(MaintenancePass::begin(root.path()).unwrap().is_none());
        drop(pass);
        let mut pass = MaintenancePass::begin(root.path()).unwrap().unwrap();
        pass.complete().unwrap();
        drop(pass);
        assert!(MaintenancePass::begin(root.path()).unwrap().is_none());
    }
}
