//! Embedded immutable assets shared by 蛹 and 舵机.

use super::local_bootstrap::{LocalHomeLayout, release_file_if_missing};
use anyhow::Result;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/embedded_local_assets.rs"));

pub fn release_embedded_local_assets(home: &Path) -> Result<usize> {
    let layout = LocalHomeLayout::from_home(home);
    layout.ensure_dirs()?;
    // The bundled files are independent immutable seeds. Publish them in a
    // small scoped worker pool so a first launch does not pay hundreds of
    // sequential directory/metadata calls. Each file still uses
    // `release_file_if_missing`, whose staging + hard-link publication keeps
    // concurrent蛹/舵机 launches atomic and never overwrites user edits.
    let workers = EMBEDDED_LOCAL_ASSETS.len().min(8).max(1);
    let chunk_size = (EMBEDDED_LOCAL_ASSETS.len() + workers - 1) / workers;
    std::thread::scope(|scope| -> Result<usize> {
        let mut jobs = Vec::new();
        for chunk in EMBEDDED_LOCAL_ASSETS.chunks(chunk_size) {
            jobs.push(scope.spawn(move || -> Result<usize> {
                let mut created = 0usize;
                for (relative, bytes) in chunk {
                    let target_relative =
                        if let Some(skill_path) = relative.strip_prefix("config/skills/") {
                            PathBuf::from("skills").join(skill_path)
                        } else {
                            PathBuf::from(relative)
                        };
                    if release_file_if_missing(&home.join(target_relative), bytes)? {
                        created += 1;
                    }
                }
                Ok(created)
            }));
        }
        let mut created = 0usize;
        for job in jobs {
            created += job
                .join()
                .map_err(|_| anyhow::anyhow!("embedded asset release worker panicked"))??;
        }
        Ok(created)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn packaged_manifest_contains_shared_config() {
        assert!(
            EMBEDDED_LOCAL_ASSETS
                .iter()
                .any(|(path, _)| *path == "config/wunder.yaml")
        );
    }

    #[test]
    fn manifest_excludes_runtime_data_and_unsafe_paths() {
        let mut seen = std::collections::HashSet::new();
        for (name, bytes) in EMBEDDED_LOCAL_ASSETS {
            assert!(seen.insert(*name), "duplicate asset: {name}");
            assert!(!bytes.is_empty(), "empty asset: {name}");
            assert!(
                Path::new(name)
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
            );
            assert!(
                *name == "config/wunder.yaml"
                    || *name == "config/i18n.messages.json"
                    || *name == "scripts/skill_runner.py"
                    || [
                        "config/prompts/",
                        "config/skills/",
                        "config/preset_worker_cards/",
                        "config/preset_worker_cards_example/"
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix)),
                "unapproved asset: {name}"
            );
            assert!(
                !name.ends_with(".db")
                    && !name.ends_with(".sqlite3")
                    && !name.ends_with(".lock")
                    && !name.contains("__pycache__")
                    && !name.contains(".env"),
                "runtime data embedded: {name}"
            );
        }
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
