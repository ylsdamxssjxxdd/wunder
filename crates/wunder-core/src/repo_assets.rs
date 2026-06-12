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
