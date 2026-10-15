//! Embedded immutable assets shared by 蛹 and 舵机.

use super::local_bootstrap::{release_file_if_missing, LocalHomeLayout};
use anyhow::Result;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/embedded_local_assets.rs"));

pub fn release_embedded_local_assets(home: &Path) -> Result<usize> {
    let layout = LocalHomeLayout::from_home(home);
    layout.ensure_dirs()?;
    let mut created = 0usize;
    for (relative, bytes) in EMBEDDED_LOCAL_ASSETS {
        let target_relative = if let Some(skill_path) = relative.strip_prefix("config/skills/") {
            PathBuf::from("skills").join(skill_path)
        } else {
            PathBuf::from(relative)
        };
        if release_file_if_missing(&home.join(target_relative), bytes)? {
            created += 1;
        }
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn packaged_manifest_contains_shared_config() {
        assert!(EMBEDDED_LOCAL_ASSETS
            .iter()
            .any(|(path, _)| *path == "config/wunder.yaml"));
    }

    #[test]
    fn release_is_idempotent() {
        let dir = tempdir().unwrap();
        let first = release_embedded_local_assets(dir.path()).unwrap();
        let second = release_embedded_local_assets(dir.path()).unwrap();
        assert!(first > 0);
        assert_eq!(second, 0);
        assert!(dir.path().join("config/wunder.yaml").is_file());
        assert!(dir.path().join("config/prompts").is_dir());
        assert!(dir.path().join("skills").is_dir());
        assert!(dir.path().join("scripts/skill_runner.py").is_file());

        std::fs::write(dir.path().join("config/wunder.yaml"), b"user-edited\n").unwrap();
        assert_eq!(release_embedded_local_assets(dir.path()).unwrap(), 0);
        assert_eq!(
            std::fs::read(dir.path().join("config/wunder.yaml")).unwrap(),
            b"user-edited\n"
        );
    }
}
