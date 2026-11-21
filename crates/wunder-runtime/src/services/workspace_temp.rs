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
}
