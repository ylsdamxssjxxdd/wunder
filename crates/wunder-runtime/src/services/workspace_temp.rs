//! Workspace scratch layout.
//!
//! Every file the runtime produces while doing work — screenshots, generated
//! media, file-history snapshots, derived chat views — belongs under the one
//! `temp/` directory at the workspace root. Keeping the layout here means a new
//! tool cannot quietly grow its own directory next to the user's deliverables.
//!
//! `temp/` is a normal workspace directory: it stays resolvable, visible in the
//! file tree, and readable by the model. It is scratch space, not a trash can —
//! callers that need a durable deliverable must take an explicit user path.

use std::io;
use std::path::{Path, PathBuf};

/// Root of all runtime scratch output, relative to the workspace root.
pub const ROOT: &str = "temp";

/// Browser screenshots (default name `browser_shot_<n>.<ext>`).
pub const BROWSER_SCREENSHOTS: &str = "temp/browser/screenshots";

/// Uploaded/original chat media.
pub const CHAT_MEDIA_SOURCE: &str = "temp/chat_media/source";

/// Derived chat views (frames, contact sheets, previews).
pub const CHAT_MEDIA_DERIVED: &str = "temp/chat_media/derived";

/// Images, speech and video produced by the generation tools.
pub const GENERATED_MEDIA: &str = "temp/generated_media";

/// Desktop-control frames copied out of the process temp root.
pub const DESKTOP_CONTROL: &str = "temp/desktop_control";

/// File-history snapshots used by write/edit/restore.
pub const FILE_HISTORY: &str = "temp/.history";

/// `temp/<kind>/<session>` for per-session scratch output.
///
/// `kind` and `session` are trimmed of slashes; an empty session collapses to
/// `temp/<kind>` instead of leaving a trailing separator.
pub fn session_dir(kind: &str, session: &str) -> String {
    let kind = kind.trim().trim_matches('/');
    let session = session.trim().trim_matches('/');
    if session.is_empty() {
        format!("{ROOT}/{kind}")
    } else {
        format!("{ROOT}/{kind}/{session}")
    }
}

/// `temp/<kind>/<session>/<file>` for a single artifact.
pub fn session_file(kind: &str, session: &str, file: &str) -> String {
    let file = file.trim().trim_matches('/');
    let dir = session_dir(kind, session);
    if file.is_empty() {
        dir
    } else {
        format!("{dir}/{file}")
    }
}

/// Whether a workspace-relative path is already inside the scratch root.
///
/// Accepts both separators so callers do not have to normalize first.
pub fn is_inside(relative: &str) -> bool {
    let normalized = relative.trim().replace('\\', "/");
    normalized == ROOT || normalized.starts_with("temp/")
}

/// How many directory entries one numbering scan will inspect. A scratch
/// directory larger than this has long stopped being useful; refusing to scan
/// the whole thing keeps a single tool call from becoming a tree walk.
const MAX_SEQUENCE_SCAN: usize = 4096;

/// How many occupied names one write will skip before giving up. The loop only
/// advances on `AlreadyExists`, so this is a concurrency guard, not a limit on
/// the directory's size.
const MAX_SEQUENCE_ATTEMPTS: u32 = 4096;

/// Next free number for `<prefix><n>` in `directory`.
///
/// Only names that are exactly the prefix plus a decimal number count. Old
/// UUID-named or hand-named files are ignored, which is what lets a directory
/// switch naming schemes without either restarting onto an existing frame or
/// feeding a 32-character hash into `u32`.
pub fn next_sequence(directory: &Path, prefix: &str) -> u32 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 1;
    };
    let mut max = 0u32;
    for entry in entries.flatten().take(MAX_SEQUENCE_SCAN) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        // The extension is not part of the sequence: `browser_shot_7.png` and
        // `browser_shot_8.jpg` still share one counter.
        let stem = rest.split('.').next().unwrap_or(rest);
        if let Ok(index) = stem.parse::<u32>() {
            max = max.max(index);
        }
    }
    max.saturating_add(1)
}

fn sequenced_name(prefix: &str, index: u32, extension: &str) -> String {
    let extension = extension.trim().trim_start_matches('.');
    if extension.is_empty() {
        format!("{prefix}{index}")
    } else {
        format!("{prefix}{index}.{extension}")
    }
}

/// Create `directory` if needed, then write `bytes` to `<prefix><n>.<ext>`.
///
/// `create_new` makes concurrent writers take different numbers instead of
/// overwriting: whoever loses the race simply moves to the next index. Returns
/// the path that was actually created.
pub fn write_sequenced_file(
    directory: &Path,
    prefix: &str,
    extension: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let mut index = next_sequence(directory, prefix);
    for _ in 0..MAX_SEQUENCE_ATTEMPTS {
        let path = directory.join(sequenced_name(prefix, index, extension));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                if let Err(err) = file.write_all(bytes) {
                    // A half-written frame must not occupy a number the next
                    // capture will read and skip.
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(err);
                }
                return Ok(path);
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                index = index.saturating_add(1);
            }
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free sequenced file name",
    ))
}

/// Write `bytes` as `<stem>_<n>.<ext>` in `directory`.
///
/// A trailing `_<n>` in `stem` is treated as the previous serial, so
/// re-uploading `screenshot_3.png` continues the directory's numbering instead
/// of producing `screenshot_3_1.png`.
pub fn write_numbered_file(
    directory: &Path,
    stem: &str,
    extension: &str,
    bytes: &[u8],
) -> io::Result<PathBuf> {
    let stem = stem.trim().trim_end_matches('_');
    let stem = without_trailing_serial(stem);
    let stem = if stem.is_empty() { "file" } else { stem };
    write_sequenced_file(directory, &format!("{stem}_"), extension, bytes)
}

/// `screenshot_12` -> `screenshot`; `IMG_2026` stays intact because a name
/// from another camera is not one of our serials, and stripping its year would
/// make every shot look like `IMG_1`.
pub fn without_trailing_serial(stem: &str) -> &str {
    let Some((base, tail)) = stem.rsplit_once('_') else {
        return stem;
    };
    if tail.is_empty() || !tail.chars().all(|ch| ch.is_ascii_digit()) {
        return stem;
    }
    let base = base.trim_end_matches('_');
    if base.is_empty() {
        return stem;
    }
    // Our own artifacts (`screenshot_3`, `photo_12`, ...) always continue
    // their counter. Foreign names only lose a small trailing number, so
    // `report_2026` still reads as 2026 instead of turning into `report_1`.
    let runtime_stem = ["screenshot", "clipboard", "photo", "image", "audio"]
        .iter()
        .any(|known| base.eq_ignore_ascii_case(known));
    if !runtime_stem {
        match tail.parse::<u32>() {
            Ok(serial) if serial <= 999 => {}
            _ => return stem,
        }
    }
    base
}

/// Create the next free `<prefix><n>` directory under `parent`.
///
/// Used for per-upload derived views: a readable `screenshot_1/` keeps the
/// frames grouped without a UUID that neither the model nor the user can
/// reference.
pub fn create_sequenced_dir(parent: &Path, prefix: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(parent)?;
    let mut index = next_sequence(parent, prefix);
    for _ in 0..MAX_SEQUENCE_ATTEMPTS {
        let path = parent.join(format!("{prefix}{index}"));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                index = index.saturating_add(1);
            }
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free sequenced directory name",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scratch_directory_lives_under_temp() {
        for path in [
            BROWSER_SCREENSHOTS,
            CHAT_MEDIA_SOURCE,
            CHAT_MEDIA_DERIVED,
            GENERATED_MEDIA,
            DESKTOP_CONTROL,
            FILE_HISTORY,
        ] {
            assert!(is_inside(path), "{path} must live under temp/");
            assert!(
                !path.trim_end_matches('/').ends_with("/temp"),
                "{path} must not be the scratch root itself"
            );
        }
    }

    #[test]
    fn session_paths_do_not_double_separators() {
        assert_eq!(
            session_dir("desktop_control", "native_1"),
            "temp/desktop_control/native_1"
        );
        assert_eq!(
            session_dir("/desktop_control/", "/native_1/"),
            "temp/desktop_control/native_1"
        );
        assert_eq!(session_dir("desktop_control", "  "), "temp/desktop_control");
        assert_eq!(
            session_file("desktop_control", "native_1", "/shot.png"),
            "temp/desktop_control/native_1/shot.png"
        );
        assert_eq!(
            session_file("desktop_control", "native_1", ""),
            "temp/desktop_control/native_1"
        );
    }

    #[test]
    fn inside_check_matches_temp_prefix_only() {
        assert!(is_inside("temp"));
        assert!(is_inside("temp/"));
        assert!(is_inside("temp/generated_media/a.png"));
        assert!(is_inside("temp\\browser\\screenshots\\a.png"));
        assert!(!is_inside("temporary/a.png"));
        assert!(!is_inside("generated_media/a.png"));
        assert!(!is_inside("/temp/a.png"));
    }

    #[test]
    fn sequence_ignores_old_uuid_names_and_shares_one_counter() {
        let dir = tempfile::tempdir().expect("temp dir");
        for name in [
            "shot_1.png",
            "shot_7.jpg",
            // 老会话的 UUID 名不参与编号，也不能把 32 位哈希当数字。
            "shot_9c5f6f0d5f1e4a0f8d3f0e7c9b2a5d10.png",
            "notes.txt",
        ] {
            std::fs::write(dir.path().join(name), b"fixture").expect("write fixture");
        }
        assert_eq!(next_sequence(dir.path(), "shot_"), 8);
    }

    #[test]
    fn sequenced_writes_never_overwrite_and_continue_the_counter() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = write_sequenced_file(dir.path(), "shot_", "png", b"one").expect("first");
        let second = write_sequenced_file(dir.path(), "shot_", "png", b"two").expect("second");
        assert_eq!(first.file_name().unwrap(), "shot_1.png");
        assert_eq!(second.file_name().unwrap(), "shot_2.png");
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
    }

    #[test]
    fn numbered_names_strip_a_previous_serial() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("screenshot_3.png"), b"old").expect("fixture");
        let path = write_numbered_file(dir.path(), "screenshot_3", "png", b"new").expect("write");
        assert_eq!(path.file_name().unwrap(), "screenshot_4.png");
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        assert_eq!(
            std::fs::read(dir.path().join("screenshot_3.png")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn numbered_names_keep_non_serial_tails() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write_numbered_file(dir.path(), "IMG_2026", "jpg", b"x").expect("write");
        assert_eq!(path.file_name().unwrap(), "IMG_2026_1.jpg");
        let path = write_numbered_file(dir.path(), "report_2026", "png", b"x").expect("write");
        assert_eq!(path.file_name().unwrap(), "report_2026_1.png");
    }

    #[test]
    fn sequenced_directories_get_readable_numbers() {
        let parent = tempfile::tempdir().expect("temp dir");
        let first = create_sequenced_dir(parent.path(), "screenshot_").expect("first");
        let second = create_sequenced_dir(parent.path(), "screenshot_").expect("second");
        assert_eq!(first.file_name().unwrap(), "screenshot_1");
        assert_eq!(second.file_name().unwrap(), "screenshot_2");
        assert!(first.is_dir() && second.is_dir());
    }
}
