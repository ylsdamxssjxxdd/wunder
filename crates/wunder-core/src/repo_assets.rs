use std::path::{Path, PathBuf};

pub const CONFIG_DIR_NAME: &str = "config";
pub const KNOWLEDGE_DIR_NAME: &str = "knowledge";
pub const PROMPTS_DIR_NAME: &str = "prompts";
pub const SKILLS_DIR_NAME: &str = "skills";

pub fn normalize_repo_root_candidate(candidate: &Path) -> PathBuf {
    if candidate
        .join(CONFIG_DIR_NAME)
        .join(PROMPTS_DIR_NAME)
        .is_dir()
        || candidate
            .join(CONFIG_DIR_NAME)
            .join("wunder.yaml")
            .is_file()
        || candidate.join(PROMPTS_DIR_NAME).is_dir()
    {
        return candidate.to_path_buf();
    }

    let file_name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if file_name.eq_ignore_ascii_case(CONFIG_DIR_NAME)
        && (candidate.join(PROMPTS_DIR_NAME).is_dir() || candidate.join("wunder.yaml").is_file())
    {
        return candidate.parent().unwrap_or(candidate).to_path_buf();
    }
    if file_name.eq_ignore_ascii_case(PROMPTS_DIR_NAME) {
        if let Some(parent) = candidate.parent() {
            let config_parent = parent
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case(CONFIG_DIR_NAME))
                .unwrap_or(false);
            if config_parent {
                return parent.parent().unwrap_or(parent).to_path_buf();
            }
            return parent.to_path_buf();
        }
    }

    candidate.to_path_buf()
}

pub fn looks_like_repo_root(candidate: &Path) -> bool {
    let normalized = normalize_repo_root_candidate(candidate);
    normalized
        .join(CONFIG_DIR_NAME)
        .join("wunder.yaml")
        .is_file()
        || normalized
            .join(CONFIG_DIR_NAME)
            .join(PROMPTS_DIR_NAME)
            .is_dir()
        || normalized.join(PROMPTS_DIR_NAME).is_dir()
}

pub fn find_repo_root_at_or_above(candidate: &Path) -> Option<PathBuf> {
    for path in candidate.ancestors() {
        let normalized = normalize_repo_root_candidate(path);
        if looks_like_repo_root(&normalized) {
            return Some(normalized);
        }
    }
    None
}

/// A real source checkout. `config/wunder.yaml` alone is not enough: a deployed
/// server leaves one in its own folder, and a loose ancestor walk would then
/// hand that foreign template (with its keys and allowed paths) to a local form.
pub fn looks_like_source_checkout(candidate: &Path) -> bool {
    looks_like_repo_root(candidate) && candidate.join("Cargo.toml").is_file()
}

fn find_source_checkout_at_or_above(candidate: &Path) -> Option<PathBuf> {
    for path in candidate.ancestors() {
        let normalized = normalize_repo_root_candidate(path);
        if looks_like_source_checkout(&normalized) {
            return Some(normalized);
        }
    }
    None
}

/// Asset root for the local forms (蜂窝 and 舵机), which ship the same layout.
/// Bundled assets next to the executable win, so a clean install never depends
/// on the folder it happens to be launched from. Source checkouts are only
/// accepted through the strict marker, keeping developer convenience without
/// inheriting an unrelated `config/wunder.yaml` from some parent directory.
pub fn resolve_local_form_repo_root(app_dir: &Path, launch_dir: Option<&Path>) -> Option<PathBuf> {
    let mut bundled = vec![app_dir.to_path_buf(), app_dir.join("resources")];
    if let Some(parent) = app_dir.parent() {
        bundled.push(parent.join("Resources"));
    }
    for candidate in &bundled {
        let normalized = normalize_repo_root_candidate(candidate);
        if looks_like_repo_root(&normalized) {
            return Some(normalized);
        }
    }
    for start in [Some(app_dir), launch_dir].into_iter().flatten() {
        if let Some(root) = find_source_checkout_at_or_above(start) {
            return Some(root);
        }
    }
    None
}

pub fn config_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(CONFIG_DIR_NAME)
}

pub fn builtin_prompts_root(repo_root: &Path) -> PathBuf {
    resolve_migrated_repo_dir(repo_root, PROMPTS_DIR_NAME)
}

pub fn default_prompt_pack_root(repo_root: &Path) -> PathBuf {
    builtin_prompts_root(repo_root)
        .parent()
        .unwrap_or(repo_root)
        .to_path_buf()
}

pub fn builtin_skills_root(repo_root: &Path) -> PathBuf {
    resolve_migrated_repo_dir(repo_root, SKILLS_DIR_NAME)
}

pub fn builtin_knowledge_root(repo_root: &Path) -> PathBuf {
    resolve_migrated_repo_dir(repo_root, KNOWLEDGE_DIR_NAME)
}

pub fn default_literal_knowledge_root(name: &str) -> String {
    format!("./{CONFIG_DIR_NAME}/{KNOWLEDGE_DIR_NAME}/{name}")
}

fn resolve_migrated_repo_dir(repo_root: &Path, name: &str) -> PathBuf {
    let migrated = config_dir(repo_root).join(name);
    let legacy = repo_root.join(name);
    if migrated.exists() || !legacy.exists() {
        migrated
    } else {
        legacy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_engine_template(root: &Path) {
        let config_dir = root.join(CONFIG_DIR_NAME);
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("wunder.yaml"),
            "server:
  mode: api
",
        )
        .expect("write template");
    }

    fn write_checkout(root: &Path) {
        fs::create_dir_all(root.join("crates")).expect("create crates dir");
        write_engine_template(root);
        fs::write(root.join("Cargo.toml"), "[workspace]
").expect("write manifest");
    }

    #[test]
    fn the_bundled_asset_root_wins_over_the_launch_folder() {
        let scratch = tempfile::tempdir().expect("scratch");
        let app_dir = scratch.path().join("app");
        let bundled = app_dir.join("resources");
        write_engine_template(&bundled);
        let checkout = scratch.path().join("checkout");
        write_checkout(&checkout);

        let resolved = resolve_local_form_repo_root(
            app_dir.as_path(),
            Some(checkout.join("crates").as_path()),
        );
        assert_eq!(resolved, Some(bundled));
    }

    #[test]
    fn a_deployed_template_alone_is_not_a_source_checkout() {
        let scratch = tempfile::tempdir().expect("scratch");
        // A deployed server leaves config/wunder.yaml in a folder that merely
        // contains the workspace; it must not become a local form's project root.
        let home = scratch.path().join("home");
        write_engine_template(&home);
        let workspace = home.join("work");
        fs::create_dir_all(&workspace).expect("create workspace");

        assert!(looks_like_repo_root(&home));
        assert!(!looks_like_source_checkout(&home));

        let app_dir = scratch.path().join("installed");
        fs::create_dir_all(&app_dir).expect("create app dir");
        let resolved = resolve_local_form_repo_root(app_dir.as_path(), Some(workspace.as_path()));
        assert_ne!(
            resolved.as_deref(),
            Some(home.as_path()),
            "a foreign deployed template must not be adopted"
        );
    }

    #[test]
    fn a_source_checkout_is_still_found_from_the_launch_folder() {
        let scratch = tempfile::tempdir().expect("scratch");
        let checkout = scratch.path().join("repo");
        write_checkout(&checkout);
        let workspace = checkout.join("crates");
        let app_dir = scratch.path().join("installed");
        fs::create_dir_all(&app_dir).expect("create app dir");

        let resolved = resolve_local_form_repo_root(app_dir.as_path(), Some(workspace.as_path()));
        assert_eq!(resolved, Some(checkout));
    }
}
