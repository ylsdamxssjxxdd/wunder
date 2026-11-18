use anyhow::{anyhow, Result};
use std::fs::{self, OpenOptions, Permissions};
use std::path::PathBuf as FsPathBuf;
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
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, destination: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // Win7-compatible same-volume atomic replacement: never expose a gap in
    // which readers could mistake the shared config/session for a logout.
    let result = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0x1 | 0x8) };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// 写入前的目标探测结果：这些属性必须在替换后原样保留。
struct TargetState {
    permissions: Permissions,
    links: u64,
}

/// 探测目标：既有的权限位与链接数；不存在（或不是普通文件）时返回 `None`。
fn inspect(target: &Path) -> Option<TargetState> {
    let meta = fs::metadata(target).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(TargetState {
        permissions: meta.permissions(),
        links: link_count(&meta),
    })
}

#[cfg(unix)]
fn link_count(meta: &fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::nlink(meta)
}

/// Windows 上 hardlink 走 rename 同样会断开，但 `std` 没有跨平台的链接数接口
/// （`number_of_links` 仅在 Windows 扩展里）；这里按 1 处理，走原子替换。
#[cfg(not(unix))]
fn link_count(_meta: &fs::Metadata) -> u64 {
    1
}

/// 写穿符号链接：rename 会把链接本身换成普通文件，用户看到的是链接失效。
fn resolve_target(target: &Path) -> FsPathBuf {
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf())
        }
        _ => target.to_path_buf(),
    }
}

/// 就地写：保留 inode，所以硬链接的另一条路径、权限位、其他属性都不变。
/// 代价是失去「先写临时文件再替换」的原子性——硬链接场景没有两全的做法，
/// 保留链接更符合用户在编辑器里的预期。
fn write_in_place(target: &Path, content: &[u8], mode: AtomicWriteMode) -> Result<()> {
    let mut file = OpenOptions::new().write(true).truncate(true).open(target)?;
    file.write_all(content)?;
    if matches!(mode, AtomicWriteMode::Strict) {
        file.sync_all()?;
    }
    Ok(())
}

/// Write bytes to `target` with an atomic swap strategy.
///
/// The implementation writes into a sibling temp file first, then replaces the
/// original path using rename semantics in the same directory.
///
/// 用户的文件不是本库的私有状态，替换时必须一并保留：
/// - 权限位（可执行脚本最容易因为「改一行」被写成 644）；
/// - 符号链接（写穿链接，别把链接本身换成普通文件）；
/// - 硬链接（目标有多条路径共享时改为就地写，否则 rename 换掉 inode，
///   另一条路径会停在旧内容上）。
pub fn atomic_write_bytes(target: &Path, content: &[u8]) -> Result<()> {
    let target = resolve_target(target);
    let target = target.as_path();
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("target path has no parent: {}", target.display()))?;
    fs::create_dir_all(parent)?;

    let mode = resolve_atomic_write_mode();
    let existing = inspect(target);
    // 硬链接：rename 换掉 inode，另一条路径会停在旧内容上，只能就地写。
    if existing.as_ref().is_some_and(|state| state.links > 1) {
        return write_in_place(target, content, mode);
    }

    let temp_path = build_temp_path(target)?;
    write_temp_file(&temp_path, content, mode)?;
    // 保留原权限位：可执行脚本（755）不能因为改一行就被写成 644。
    if let Some(state) = existing.as_ref() {
        if let Err(err) = fs::set_permissions(&temp_path, state.permissions.clone()) {
            cleanup(&temp_path);
            return Err(err.into());
        }
    }

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

    #[test]
    fn atomic_write_creates_nested_parent_directories() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("deeper").join("note.txt");

        atomic_write_text(&target, "nested").expect("write");

        let saved = fs::read_to_string(&target).expect("read");
        assert_eq!(saved, "nested");
    }

    #[test]
    fn atomic_write_leaves_no_sibling_temp_files_after_success() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("note.txt");

        atomic_write_text(&target, "clean").expect("write");

        let leftovers = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".wunder.tmp."))
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_mode_bits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("run.sh");
        fs::write(&target, "#!/bin/sh\necho old\n").expect("seed");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).expect("chmod");

        atomic_write_text(&target, "#!/bin/sh\necho new\n").expect("write");

        let mode = fs::metadata(&target).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "改写不能吃掉可执行位");
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_keeps_hard_links_in_sync() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("run.sh");
        let alias = dir.path().join("alias.sh");
        fs::write(&target, "old\n").expect("seed");
        fs::hard_link(&target, &alias).expect("link");
        let inode = fs::metadata(&target).expect("meta").ino();

        atomic_write_text(&target, "new\n").expect("write");

        assert_eq!(
            fs::metadata(&target).expect("meta").ino(),
            inode,
            "硬链接场景必须保留 inode"
        );
        assert_eq!(
            fs::read_to_string(&alias).expect("read alias"),
            "new\n",
            "另一条路径要看到同一份内容"
        );
        assert_eq!(fs::metadata(&alias).expect("meta").nlink(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_follows_symlinks_instead_of_replacing_them() {
        let dir = tempdir().expect("tempdir");
        let real = dir.path().join("real.txt");
        let link = dir.path().join("link.txt");
        fs::write(&real, "old\n").expect("seed");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        atomic_write_text(&link, "new\n").expect("write");

        assert!(
            fs::symlink_metadata(&link)
                .expect("meta")
                .file_type()
                .is_symlink(),
            "链接本身必须还在"
        );
        assert_eq!(fs::read_to_string(&real).expect("read real"), "new\n");
    }
}
