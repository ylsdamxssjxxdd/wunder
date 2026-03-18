use crate::services::default_agent_sync::DEFAULT_AGENT_ID_ALIAS;
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use crate::workspace::WorkspaceManager;
use std::path::PathBuf;

pub const INNER_VISIBLE_DIR: &str = ".wunder";
pub const EFFECTIVE_DIR: &str = "effective";
pub const DIAGNOSTICS_DIR: &str = "diagnostics";
pub const LAST_GOOD_DIR: &str = "last_good";
pub const GLOBAL_DIR: &str = "global";
pub const AGENTS_DIR: &str = "agents";
pub const SKILLS_DIR: &str = "skills";
pub const KNOWLEDGE_DIR: &str = "knowledge";
pub const TOOLING_FILE: &str = "tooling.json";
pub const DEFAULTS_WORKER_CARD_FILE: &str = "defaults.worker-card.json";
pub const WORKER_CARD_FILE: &str = "worker-card.json";
pub const SYSTEM_PROMPT_FILE: &str = "system_prompt.md";

#[derive(Debug, Clone)]
pub struct InnerVisiblePaths {
    pub private_root: PathBuf,
    pub global_dir: PathBuf,
    pub agents_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub knowledge_dir: PathBuf,
    pub inner_visible_dir: PathBuf,
    pub effective_dir: PathBuf,
    pub diagnostics_dir: PathBuf,
    pub last_good_dir: PathBuf,
}

pub fn user_paths(workspace: &WorkspaceManager, user_id: &str) -> InnerVisiblePaths {
    let private_root = private_root(workspace, user_id);
    let global_dir = private_root.join(GLOBAL_DIR);
    let agents_dir = private_root.join(AGENTS_DIR);
    let inner_visible_dir = private_root.join(INNER_VISIBLE_DIR);
    InnerVisiblePaths {
        skills_dir: private_root.join(SKILLS_DIR),
        knowledge_dir: private_root.join(KNOWLEDGE_DIR),
        effective_dir: inner_visible_dir.join(EFFECTIVE_DIR),
        diagnostics_dir: inner_visible_dir.join(DIAGNOSTICS_DIR),
        last_good_dir: inner_visible_dir.join(LAST_GOOD_DIR),
        private_root,
        global_dir,
        agents_dir,
        inner_visible_dir,
    }
}

pub fn private_root(workspace: &WorkspaceManager, user_id: &str) -> PathBuf {
    let scoped_user_id = workspace.scoped_user_id_by_container(user_id, USER_PRIVATE_CONTAINER_ID);
    workspace.workspace_root(&scoped_user_id)
}

pub fn normalize_agent_dir_name(agent_id: Option<&str>) -> String {
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

pub fn agent_dir(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> PathBuf {
    paths.agents_dir.join(normalize_agent_dir_name(agent_id))
}

pub fn worker_card_path(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> PathBuf {
    agent_dir(paths, agent_id).join(WORKER_CARD_FILE)
}

pub fn system_prompt_path(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> PathBuf {
    agent_dir(paths, agent_id).join(SYSTEM_PROMPT_FILE)
}

pub fn tooling_path(paths: &InnerVisiblePaths) -> PathBuf {
    paths.global_dir.join(TOOLING_FILE)
}

pub fn defaults_worker_card_path(paths: &InnerVisiblePaths) -> PathBuf {
    paths.global_dir.join(DEFAULTS_WORKER_CARD_FILE)
}

pub fn global_effective_path(paths: &InnerVisiblePaths) -> PathBuf {
    paths.effective_dir.join("global.json")
}

pub fn agent_effective_path(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> PathBuf {
    paths
        .effective_dir
        .join(AGENTS_DIR)
        .join(format!("{}.json", normalize_agent_dir_name(agent_id)))
}

pub fn global_diagnostics_path(paths: &InnerVisiblePaths) -> PathBuf {
    paths.diagnostics_dir.join("global.json")
}

pub fn agent_diagnostics_path(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> PathBuf {
    paths
        .diagnostics_dir
        .join(AGENTS_DIR)
        .join(format!("{}.json", normalize_agent_dir_name(agent_id)))
}

pub fn last_good_agent_dir(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> PathBuf {
    paths
        .last_good_dir
        .join(AGENTS_DIR)
        .join(normalize_agent_dir_name(agent_id))
}

