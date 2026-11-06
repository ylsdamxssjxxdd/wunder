//! Embedded immutable assets shared by 蛹 and 舵机.

use super::local_bootstrap::{
    release_assets_with_id, LocalHomeLayout, ReleasePolicy, ReleaseReport,
};
use anyhow::Result;
use std::path::Path;

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

/// Release bundled assets, returning how many files were created, replaced or
/// retired. Kept for startup call sites that only log a count.
pub fn release_embedded_local_assets(home: &Path) -> Result<usize> {
    Ok(release_embedded_local_assets_with_report(home)?.changed())
}

/// Release bundled assets with three-state semantics:
///
/// * untouched system copies follow the build,
/// * user-edited copies are preserved (new bytes go to `temp/asset-conflicts/`),
/// * `config/wunder.yaml` is only seeded when absent.
pub fn release_embedded_local_assets_with_report(home: &Path) -> Result<ReleaseReport> {
    let layout = LocalHomeLayout::from_home(home);
    layout.ensure_dirs()?;

    let mut managed: Vec<(String, &'static [u8])> = Vec::new();
    let mut seeds: Vec<(String, &'static [u8])> = Vec::new();
    for entry in EMBEDDED_LOCAL_ASSETS {
        let (relative, bytes) = *entry;
        // The user configuration is never replaced; it is only created on the
        // first launch and later merged key-by-key.
        if relative == "config/wunder.yaml" {
            seeds.push((relative.to_string(), bytes));
            continue;
        }
        // Immutable skills live under the runtime skills root, matching the
        // path resolution performed after the first launch.
        let target = match relative.strip_prefix("config/skills/") {
            Some(rest) => format!("skills/{rest}"),
            None => relative.to_string(),
        };
        managed.push((target, bytes));
    }

    release_assets_with_id(
        home,
        &managed,
        &seeds,
        &ReleasePolicy::default(),
        Some(EMBEDDED_ASSET_SET_ID),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::local_bootstrap::ReleaseAction;
    use std::fs;
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
                    || *name == "config/wunder-example.yaml"
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
        assert!(dir.path().join("config/wunder-example.yaml").is_file());
        assert!(dir.path().join("config/prompts").is_dir());
        assert!(dir.path().join("skills").is_dir());
        assert!(dir.path().join("scripts/skill_runner.py").is_file());

        fs::write(dir.path().join("config/wunder.yaml"), b"user-edited\n").unwrap();
        assert_eq!(release_embedded_local_assets(dir.path()).unwrap(), 0);
        assert_eq!(
            fs::read(dir.path().join("config/wunder.yaml")).unwrap(),
            b"user-edited\n"
        );
    }

    #[test]
    fn untouched_prompt_follows_the_build_and_user_edit_is_kept() {
        let dir = tempdir().unwrap();
        let prompt = dir.path();
        let v1: &'static [u8] = b"system prompt v1";
        let v2: &'static [u8] = b"system prompt v2";
        let managed = vec![("config/prompts/zh/role.txt".to_string(), v1)];

        let report = release_assets(prompt, &managed, &[], &ReleasePolicy::default()).unwrap();
        assert_eq!(report.count(ReleaseAction::Created), 1);

        // Untouched copy follows the new build.
        let managed_v2 = vec![("config/prompts/zh/role.txt".to_string(), v2)];
        let report = release_assets(prompt, &managed_v2, &[], &ReleasePolicy::default()).unwrap();
        assert_eq!(report.count(ReleaseAction::Updated), 1);
        assert_eq!(
            fs::read(prompt.join("config/prompts/zh/role.txt")).unwrap(),
            v2
        );

        // A user edit survives the next build and yields a conflict copy of the
        // new bytes for manual comparison.
        fs::write(prompt.join("config/prompts/zh/role.txt"), b"my own prompt").unwrap();
        let v3: &'static [u8] = b"system prompt v3";
        let managed_v3 = vec![("config/prompts/zh/role.txt".to_string(), v3)];
        let report = release_assets(prompt, &managed_v3, &[], &ReleasePolicy::default()).unwrap();
        assert_eq!(report.count(ReleaseAction::KeptUserModified), 1);
        assert_eq!(report.conflict_count(), 1);
        assert_eq!(
            fs::read(prompt.join("config/prompts/zh/role.txt")).unwrap(),
            b"my own prompt"
        );
        let conflict = &report.conflicts[0];
        assert_eq!(fs::read(conflict).unwrap(), v3);
    }

    #[test]
    fn user_deletion_is_respected_and_retired_assets_are_removed() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first: &'static [u8] = b"first";
        let second: &'static [u8] = b"second";
        let managed = vec![
            ("config/prompts/zh/role.txt".to_string(), first),
            ("config/prompts/zh/retired.txt".to_string(), second),
        ];
        release_assets(root, &managed, &[], &ReleasePolicy::default()).unwrap();

        fs::remove_file(root.join("config/prompts/zh/role.txt")).unwrap();
        let report = release_assets(root, &managed, &[], &ReleasePolicy::default()).unwrap();
        assert_eq!(report.count(ReleaseAction::KeptUserDeleted), 1);
        assert!(!root.join("config/prompts/zh/role.txt").exists());

        // Dropping an untouched asset from the build retires it on disk.
        let only_role = vec![("config/prompts/zh/role.txt".to_string(), first)];
        let report = release_assets(root, &only_role, &[], &ReleasePolicy::default()).unwrap();
        assert_eq!(report.count(ReleaseAction::Removed), 1);
        assert!(!root.join("config/prompts/zh/retired.txt").exists());
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
                "missing spritesheet for {id}"
            );
        }
        assert!(released > 0, "no bundled companions were embedded");
    }
}