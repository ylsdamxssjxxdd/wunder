use anyhow::{anyhow, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);
static ATOMIC_WRITE_MODE: OnceLock<AtomicWriteMode> = OnceLock::new();
const ATOMIC_WRITE_MODE_ENV: &str = "WUNDER_ATOMIC_WRITE_MODE";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AtomicWriteMode {
    Strict,
    Relaxed,
}

fn unique_suffix() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|item| item.as_nanos() as u64)
        .unwrap_or(0);
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    nanos ^ counter.rotate_left(13) ^ (std::process::id() as u64).rotate_left(27)
}

fn build_temp_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target path has no parent: {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|item| item.to_str())
        .ok_or_else(|| anyhow!("target path has invalid file name: {}", path.display()))?;
    let suffix = unique_suffix();
    Ok(parent.join(format!(".{name}.wunder.tmp.{suffix:x}")))
}

#[cfg(windows)]
fn build_backup_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target path has no parent: {}", path.display()))?;
    let name = path
        .file_name()
        .and_then(|item| item.to_str())
        .ok_or_else(|| anyhow!("target path has invalid file name: {}", path.display()))?;
    let suffix = unique_suffix();
    Ok(parent.join(format!(".{name}.wunder.bak.{suffix:x}")))
}

fn cleanup(path: &Path) {
    let _ = fs::remove_file(path);
}

fn resolve_atomic_write_mode() -> AtomicWriteMode {
    *ATOMIC_WRITE_MODE.get_or_init(|| match std::env::var(ATOMIC_WRITE_MODE_ENV) {
        Ok(raw) if raw.trim().eq_ignore_ascii_case("relaxed") => AtomicWriteMode::Relaxed,
        _ => AtomicWriteMode::Strict,
    })
}

fn write_temp_file(temp_path: &Path, content: &[u8], mode: AtomicWriteMode) -> Result<()> {
    let mut temp_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temp_path)?;
    temp_file.write_all(content)?;
    if matches!(mode, AtomicWriteMode::Strict) {
        temp_file.sync_all()?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_target(temp_path: &Path, target: &Path) -> Result<()> {
    fs::rename(temp_path, target)?;
    Ok(())
}

#[cfg(windows)]
fn replace_target(temp_path: &Path, target: &Path) -> Result<()> {
    if !target.exists() {
        fs::rename(temp_path, target)?;
        return Ok(());
    }
    let backup_path = build_backup_path(target)?;
    fs::rename(target, &backup_path)?;
    match fs::rename(temp_path, target) {
        Ok(()) => {
            cleanup(&backup_path);
            Ok(())
        }
        Err(err) => {
            let rollback_result = fs::rename(&backup_path, target);
            cleanup(temp_path);
            match rollback_result {
                Ok(()) => Err(anyhow!(
                    "failed to replace target file atomically: {err}; rollback restored original file"
                )),
                Err(rollback_err) => Err(anyhow!(
                    "failed to replace target file atomically: {err}; rollback failed: {rollback_err}"
                )),
            }
        }
    }
}

/// Write text content to `target` with an atomic swap strategy.
///
/// The implementation writes into a sibling temp file first, then replaces the
/// original path using rename semantics in the same directory.
pub fn atomic_write_bytes(target: &Path, content: &[u8]) -> Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("target path has no parent: {}", target.display()))?;
    fs::create_dir_all(parent)?;

    let mode = resolve_atomic_write_mode();
    let temp_path = build_temp_path(target)?;
    write_temp_file(&temp_path, content, mode)?;

    if let Err(err) = replace_target(&temp_path, target) {
        cleanup(&temp_path);
        return Err(err);
    }
    Ok(())
}

pub fn atomic_write_text(target: &Path, content: &str) -> Result<()> {
    atomic_write_bytes(target, content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolve_atomic_write_mode_defaults_to_strict() {
        assert_eq!(resolve_atomic_write_mode(), AtomicWriteMode::Strict);
    }

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("note.txt");
        atomic_write_text(&target, "hello").expect("write");
        let saved = fs::read_to_string(&target).expect("read");
        assert_eq!(saved, "hello");
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("note.txt");
        fs::write(&target, "old").expect("seed");
        atomic_write_text(&target, "new").expect("write");
        let saved = fs::read_to_string(&target).expect("read");
        assert_eq!(saved, "new");
    }
}
