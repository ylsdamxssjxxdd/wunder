use crate::services::default_agent_sync::DEFAULT_AGENT_ID_ALIAS;
use crate::services::worker_card_files::{
    stable_id_from_worker_card_file_name, worker_card_file_name as canonical_worker_card_file_name,
};
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use crate::workspace::WorkspaceManager;
use std::path::PathBuf;

pub const GLOBAL_DIR: &str = "global";
pub const AGENTS_DIR: &str = "agents";
pub const SKILLS_DIR: &str = "skills";
pub const KNOWLEDGE_DIR: &str = "knowledge";
pub const TOOLING_FILE: &str = "tooling.json";
pub const DEFAULTS_WORKER_CARD_FILE: &str = "defaults.worker-card.json";
pub const LEGACY_INNER_VISIBLE_DIR: &str = ".wunder";

#[derive(Debug, Clone)]
pub struct InnerVisiblePaths {
    pub private_root: PathBuf,
    pub global_dir: PathBuf,
    pub agents_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub knowledge_dir: PathBuf,
    pub legacy_inner_visible_dir: PathBuf,
}

pub fn user_paths(workspace: &WorkspaceManager, user_id: &str) -> InnerVisiblePaths {
    let private_root = private_root(workspace, user_id);
    InnerVisiblePaths {
        global_dir: private_root.join(GLOBAL_DIR),
        agents_dir: private_root.join(AGENTS_DIR),
        skills_dir: private_root.join(SKILLS_DIR),
        knowledge_dir: private_root.join(KNOWLEDGE_DIR),
        legacy_inner_visible_dir: private_root.join(LEGACY_INNER_VISIBLE_DIR),
        private_root,
    }
}

pub fn private_root(workspace: &WorkspaceManager, user_id: &str) -> PathBuf {
    let scoped_user_id = workspace.scoped_user_id_by_container(user_id, USER_PRIVATE_CONTAINER_ID);
    workspace.workspace_root(&scoped_user_id)
}

pub fn normalize_agent_file_stem(agent_id: Option<&str>) -> String {
    let cleaned = agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_AGENT_ID_ALIAS);
    if cleaned.eq_ignore_ascii_case("default") {
        DEFAULT_AGENT_ID_ALIAS.to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn worker_card_file_name(display_name: Option<&str>, agent_id: Option<&str>) -> String {
    canonical_worker_card_file_name(display_name, Some(&normalize_agent_file_stem(agent_id)))
}

pub fn worker_card_path(
    paths: &InnerVisiblePaths,
    display_name: Option<&str>,
    agent_id: Option<&str>,
) -> PathBuf {
    paths.agents_dir
        .join(worker_card_file_name(display_name, agent_id))
}

pub fn tooling_path(paths: &InnerVisiblePaths) -> PathBuf {
    paths.global_dir.join(TOOLING_FILE)
}

pub fn defaults_worker_card_path(paths: &InnerVisiblePaths) -> PathBuf {
    paths.global_dir.join(DEFAULTS_WORKER_CARD_FILE)
}

pub fn agent_id_from_worker_card_file_name(file_name: &str) -> Option<String> {
    stable_id_from_worker_card_file_name(file_name)
        .map(|stable_id| normalize_agent_file_stem(Some(&stable_id)))
}
