use crate::a2a_store::A2aStore;
use crate::config::Config;
use crate::cron::CronWakeSignal;
use crate::gateway::GatewayHub;
use crate::lsp::LspManager;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::services::beeroom_realtime::BeeroomRealtimeService;
use crate::skills::SkillRegistry;
use crate::storage::StorageBackend;
use crate::user_tools::{UserToolBindings, UserToolManager, UserToolStore};
use crate::user_world::UserWorldService;
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

type ToolEventCallback = dyn Fn(&str, Value) + Send + Sync;

#[derive(Clone)]
pub struct ToolEventEmitter {
    callback: Arc<ToolEventCallback>,
    stream: bool,
}

impl ToolEventEmitter {
    pub fn new<F>(callback: F, stream: bool) -> Self
    where
        F: Fn(&str, Value) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
            stream,
        }
    }

    pub fn emit(&self, event_type: &str, data: Value) {
        (self.callback)(event_type, data);
    }

    pub fn stream_enabled(&self) -> bool {
        self.stream
    }
}

pub struct ToolContext<'a> {
    pub user_id: &'a str,
    pub session_id: &'a str,
    pub workspace_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub is_admin: bool,
    pub storage: Arc<dyn StorageBackend>,
    pub orchestrator: Option<Arc<Orchestrator>>,
    pub monitor: Option<Arc<MonitorState>>,
    pub beeroom_realtime: Option<Arc<BeeroomRealtimeService>>,
    pub workspace: Arc<WorkspaceManager>,
    pub lsp_manager: Arc<LspManager>,
    pub config: &'a Config,
    pub a2a_store: &'a A2aStore,
    pub skills: &'a SkillRegistry,
    pub gateway: Option<Arc<GatewayHub>>,
    pub user_world: Option<Arc<UserWorldService>>,
    pub cron_wake_signal: Option<CronWakeSignal>,
    pub user_tool_manager: Option<Arc<UserToolManager>>,
    pub user_tool_bindings: Option<&'a UserToolBindings>,
    pub user_tool_store: Option<&'a UserToolStore>,
    pub request_config_overrides: Option<&'a Value>,
    pub allow_roots: Option<Arc<Vec<PathBuf>>>,
    pub read_roots: Option<Arc<Vec<PathBuf>>>,
    pub event_emitter: Option<ToolEventEmitter>,
    pub http: &'a reqwest::Client,
}

#[derive(Clone)]
pub struct ToolRoots {
    pub allow_roots: Arc<Vec<PathBuf>>,
    pub read_roots: Arc<Vec<PathBuf>>,
}

pub fn build_tool_roots(
    config: &Config,
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
) -> ToolRoots {
    let allow_roots = build_allow_roots(config);
    let mut read_roots = allow_roots.clone();
    read_roots.extend(build_skill_roots(skills, user_tool_bindings));
    let read_roots = dedupe_roots(read_roots);
    ToolRoots {
        allow_roots: Arc::new(allow_roots),
        read_roots: Arc::new(read_roots),
    }
}

pub(crate) fn dedupe_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for root in roots {
        let normalized = normalize_existing_path(&root);
        let key = normalize_path_for_compare(&normalized);
        if seen.insert(key) {
            output.push(normalized);
        }
    }
    output
}

pub(crate) fn build_allow_roots(config: &Config) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let cwd = std::env::current_dir().ok();
    for raw in &config.security.allow_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = PathBuf::from(trimmed);
        let resolved = if path.is_absolute() {
            path
        } else if let Some(base) = &cwd {
            base.join(path)
        } else {
            path
        };
        roots.push(resolved);
    }
    // Desktop local mode should be able to inspect and operate on the local filesystem
    // without being artificially constrained to the per-user workspace root.
    if is_desktop_local_mode(config) {
        roots.extend(desktop_local_allow_roots());
    }
    dedupe_roots(roots)
}

fn is_desktop_local_mode(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop")
        && config.sandbox.mode.trim().eq_ignore_ascii_case("local")
}

#[cfg(windows)]
fn desktop_local_allow_roots() -> Vec<PathBuf> {
    ('A'..='Z')
        .map(|drive| PathBuf::from(format!("{drive}:\\")))
        .filter(|path| path.exists())
        .collect()
}

#[cfg(not(windows))]
fn desktop_local_allow_roots() -> Vec<PathBuf> {
    vec![PathBuf::from(std::path::MAIN_SEPARATOR.to_string())]
}

fn path_is_filesystem_root(path: &Path) -> bool {
    path.parent().is_none()
}

fn allow_any_path_in_roots(roots: &[PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| path_is_filesystem_root(root.as_path()))
}

pub(crate) fn collect_allow_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    if let Some(roots) = context.allow_roots.as_ref() {
        return roots.as_ref().clone();
    }
    build_allow_roots(context.config)
}

pub(crate) fn collect_read_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    if let Some(roots) = context.read_roots.as_ref() {
        return roots.as_ref().clone();
    }
    let mut roots = collect_allow_roots(context);
    roots.extend(collect_skill_roots(context));
    dedupe_roots(roots)
}

pub(crate) fn collect_skill_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    build_skill_roots(context.skills, context.user_tool_bindings)
}

pub(crate) fn build_skill_roots(
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = skills
        .list_specs()
        .into_iter()
        .map(|spec| spec.root)
        .collect();
    if let Some(bindings) = user_tool_bindings {
        for source in bindings.skill_sources.values() {
            roots.push(source.root.clone());
        }
    }
    roots
}

pub(crate) fn resolve_path_in_roots(raw_path: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let allow_any_path = allow_any_path_in_roots(roots);
    let candidate = {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else if allow_any_path {
            // When local desktop mode exposes filesystem roots, keep relative traversal available
            // so agent tools can follow user-provided local paths instead of failing on `..`.
            let cwd = std::env::current_dir().ok()?;
            normalize_target_path(&cwd.join(path))
        } else {
            let relative = sanitize_relative_path(trimmed)?;
            let cwd = std::env::current_dir().ok()?;
            cwd.join(relative)
        }
    };
    for root in roots {
        if is_within_root(root, &candidate) {
            return Some(candidate.clone());
        }
    }
    None
}

pub(crate) fn resolve_tool_path(
    workspace: &WorkspaceManager,
    user_id: &str,
    raw_path: &str,
    extra_roots: &[PathBuf],
) -> Result<PathBuf> {
    match workspace.resolve_path(user_id, raw_path) {
        Ok(path) => Ok(path),
        Err(err) => {
            if let Some(resolved) = resolve_path_in_roots(raw_path, extra_roots) {
                Ok(resolved)
            } else {
                Err(err)
            }
        }
    }
}

pub(crate) fn sanitize_relative_path(raw_path: &str) -> Option<PathBuf> {
    let normalized = raw_path.trim().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    if stripped.is_empty() {
        return None;
    }
    let path = PathBuf::from(stripped);
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return None;
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::{build_allow_roots, resolve_path_in_roots};
    use crate::config::Config;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn filesystem_root_for(path: &Path) -> PathBuf {
        path.ancestors()
            .last()
            .expect("filesystem root")
            .to_path_buf()
    }

    #[test]
    fn desktop_local_mode_includes_filesystem_roots() {
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        config.sandbox.mode = "local".to_string();

        let roots = build_allow_roots(&config);

        assert!(roots.iter().any(|root| root.parent().is_none()));
    }

    #[test]
    fn resolve_path_in_roots_allows_absolute_path_under_filesystem_root() {
        let temp = tempdir().expect("tempdir");
        let target = temp.path().join("outside.txt");
        fs::write(&target, "ok").expect("write target");
        let roots = vec![filesystem_root_for(&target)];

        let resolved = resolve_path_in_roots(&target.to_string_lossy(), &roots).expect("resolved");

        assert_eq!(resolved, target);
    }
}
