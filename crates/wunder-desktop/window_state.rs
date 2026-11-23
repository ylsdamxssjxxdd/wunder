//! Persisted placement of the desktop shell's main window.
//!
//! The shell reads this before the engine finishes starting, so it is kept in
//! its own file instead of `desktop.settings.json`: the runtime rewrites that
//! file wholesale on startup and on every settings save, and a placement
//! written from the window code would race those writes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// File name under the local home's `config` directory.
pub const FILE_NAME: &str = "desktop.window.json";

/// Main-window placement in physical pixels. `x`/`y` are the outer top-left
/// corner; `width`/`height` are the inner size, which equals the outer size of
/// the frameless workbench. A stored frame without a size carries only the
/// maximized state (first session that ended maximized after a fresh install).
///
/// `pinned` is the chrome's always-on-top toggle. It rides along with the
/// frame because it is window state, not app settings: it changes at exactly
/// the moment the window is on screen, and it has to be re-applied to the
/// native window at map time, before the user touches anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub maximized: bool,
    #[serde(default)]
    pub pinned: bool,
}

impl WindowGeometry {
    pub fn has_frame(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    fn is_plausible(&self) -> bool {
        (self.has_frame() || self.maximized)
            && self.width <= 16384
            && self.height <= 16384
            && self.x.unsigned_abs() <= 1_000_000
            && self.y.unsigned_abs() <= 1_000_000
    }
}

/// The placement saved by the last session, if it is still readable.
pub fn load() -> Option<WindowGeometry> {
    load_from(&file_path())
}

/// Best effort: a placement that cannot be written must never break the shell.
pub fn save(geometry: &WindowGeometry) {
    if !geometry.is_plausible() {
        return;
    }
    let _ = save_to(&file_path(), geometry);
}

pub fn file_path() -> PathBuf {
    crate::local_config_dir().join(FILE_NAME)
}

fn load_from(path: &Path) -> Option<WindowGeometry> {
    let text = std::fs::read_to_string(path).ok()?;
    let geometry: WindowGeometry = serde_json::from_str(&text).ok()?;
    geometry.is_plausible().then_some(geometry)
}

fn save_to(path: &Path, geometry: &WindowGeometry) -> Result<()> {
    let text = serde_json::to_string_pretty(geometry).context("serialize window placement")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create window placement dir: {}", parent.display())
        })?;
    }
    let temp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp_path, text)
        .with_context(|| format!("write window placement: {}", temp_path.display()))?;
    if let Err(initial_err) = std::fs::rename(&temp_path, path) {
        if let Err(remove_err) = std::fs::remove_file(path) {
            if remove_err.kind() != std::io::ErrorKind::NotFound {
                return Err(remove_err).context("remove old window placement");
            }
        }
        std::fs::rename(&temp_path, path).with_context(|| {
            format!(
                "replace window placement: {} (initial rename error: {initial_err})",
                path.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WindowGeometry {
        WindowGeometry {
            x: -40,
            y: 120,
            width: 960,
            height: 780,
            maximized: false,
            pinned: false,
        }
    }

    #[test]
    fn placement_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        save_to(&path, &sample()).unwrap();
        assert_eq!(load_from(&path), Some(sample()));
    }

    #[test]
    fn pin_state_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let pinned = WindowGeometry {
            pinned: true,
            ..sample()
        };
        save_to(&path, &pinned).unwrap();
        assert_eq!(load_from(&path), Some(pinned));
    }

    #[test]
    fn a_placement_from_before_the_pin_reads_as_unpinned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, r#"{"x":10,"y":20,"width":800,"height":600}"#).unwrap();
        let loaded = load_from(&path).expect("legacy placement still loads");
        assert!(!loaded.pinned);
        assert!(!loaded.maximized);
    }

    #[test]
    fn frameless_only_placement_carries_the_maximized_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let maximized = WindowGeometry {
            width: 0,
            height: 0,
            maximized: true,
            ..sample()
        };
        save_to(&path, &maximized).unwrap();
        assert_eq!(load_from(&path), Some(maximized));
    }

    #[test]
    fn a_frame_without_size_or_maximized_flag_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let blank = WindowGeometry {
            width: 0,
            height: 0,
            maximized: false,
            ..sample()
        };
        save_to(&path, &blank).unwrap();
        assert_eq!(load_from(&path), None);
    }

    #[test]
    fn corrupt_content_reads_as_no_placement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path), None);
    }
}