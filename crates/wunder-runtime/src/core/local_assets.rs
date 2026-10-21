//! Embedded immutable assets shared by 蛹 and 舵机.

use super::local_bootstrap::{release_file_if_missing, LocalHomeLayout};
use anyhow::Result;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/embedded_local_assets.rs"));

/// Immutable build-time asset content, addressed by its projected home path.
/// The engine template ships as `config/wunder.yaml` so a first launch can seed
/// the writable config without a second copy next to the executable.
pub fn embedded_asset(name: &str) -> Option<&'static [u8]> {
    EMBEDDED_LOCAL_ASSETS
        .iter()
        .find(|(path, _)| *path == name)
        .map(|(_, bytes)| *bytes)
}

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
        assert!(EMBEDDED_LOCAL_ASSETS
            .iter()
            .any(|(path, _)| *path == "config/wunder.yaml"));
        assert!(embedded_asset("config/wunder.yaml")
            .map(|bytes| !bytes.is_empty())
            .unwrap_or(false));
    }

    #[test]
    fn manifest_excludes_runtime_data_and_unsafe_paths() {
        let mut seen = std::collections::HashSet::new();
        for (name, bytes) in EMBEDDED_LOCAL_ASSETS {
            assert!(seen.insert(*name), "duplicate asset: {name}");
            assert!(!bytes.is_empty(), "empty asset: {name}");
            assert!(Path::new(name)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))));
            assert!(
                *name == "config/wunder.yaml"
                    || *name == "config/i18n.messages.json"
                    || *name == "scripts/skill_runner.py"
                    || [
                        "config/prompts/",
                        "config/skills/",
                        "config/preset_worker_cards/",
                        "config/preset_worker_cards_example/",
                        "config/data/companions/global/"
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

    #[test]
    fn bundled_companions_are_released_into_the_library_root() {
        let dir = tempdir().unwrap();
        release_embedded_local_assets(dir.path()).unwrap();
        let root = dir.path().join("config/data/companions/global");
        let mut released = 0usize;
        for &(name, bytes) in EMBEDDED_LOCAL_ASSETS {
            if !name.starts_with("config/data/companions/global/") || !name.ends_with("/pet.json") {
                continue;
            }
            released += 1;
            assert!(dir.path().join(name).is_file(), "missing released {name}");
            let manifest: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            let sheet = manifest["spritesheetPath"].as_str().unwrap();
            let id = name
                .strip_prefix("config/data/companions/global/")
                .unwrap()
                .split('/')
                .next()
                .unwrap();
            assert_eq!(
                manifest["id"].as_str().unwrap(),
                id,
                "companion id must match its library directory"
            );
            assert!(
                root.join(id).join(sheet).is_file(),
                "missing spritesheet for {name}"
            );
        }
        assert!(released > 0, "no bundled companions were embedded");
    }
}
