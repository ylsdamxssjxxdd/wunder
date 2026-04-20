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
use crate::services::tools::command_sessions::CommandSessionBroker;
use crate::skills::SkillRegistry;
use crate::storage::StorageBackend;
use crate::user_tools::{UserToolBindings, UserToolManager, UserToolStore};
use crate::user_world::UserWorldService;
use crate::workspace::WorkspaceManager;
use crate::services::orchestration_context::{parse_round_index_token, round_dir_aliases};
use anyhow::Result;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

type ToolEventCallback = dyn Fn(&str, Value) + Send + Sync;

#[derive(Clone)]
pub struct ToolEventEmitter {
    callback: Arc<ToolEventCallback>,
    stream: bool,
    default_fields: Arc<Map<String, Value>>,
}

impl ToolEventEmitter {
    pub fn new<F>(callback: F, stream: bool) -> Self
    where
        F: Fn(&str, Value) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
            stream,
            default_fields: Arc::new(Map::new()),
        }
    }

    pub fn with_field(&self, key: impl Into<String>, value: Value) -> Self {
        let key = key.into();
        if key.trim().is_empty() {
            return self.clone();
        }
        let mut default_fields = self.default_fields.as_ref().clone();
        default_fields.insert(key, value);
        Self {
            callback: Arc::clone(&self.callback),
            stream: self.stream,
            default_fields: Arc::new(default_fields),
        }
    }

    pub fn emit(&self, event_type: &str, mut data: Value) {
        if !self.default_fields.is_empty() {
            if let Value::Object(ref mut map) = data {
                for (key, value) in self.default_fields.iter() {
                    map.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        (self.callback)(event_type, data);
    }

    pub fn stream_enabled(&self) -> bool {
        self.stream
    }

    pub fn default_string_field(&self, key: &str) -> Option<String> {
        self.default_fields
            .get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    }
}

pub struct ToolContext<'a> {
    pub user_id: &'a str,
    pub session_id: &'a str,
    pub workspace_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub user_round: Option<i64>,
    pub model_round: Option<i64>,
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
    pub command_sessions: Option<Arc<CommandSessionBroker>>,
    pub event_emitter: Option<ToolEventEmitter>,
    pub http: &'a reqwest::Client,
}

impl<'a> ToolContext<'a> {
    pub fn with_event_emitter(&self, event_emitter: Option<ToolEventEmitter>) -> ToolContext<'a> {
        ToolContext {
            user_id: self.user_id,
            session_id: self.session_id,
            workspace_id: self.workspace_id,
            agent_id: self.agent_id,
            user_round: self.user_round,
            model_round: self.model_round,
            is_admin: self.is_admin,
            storage: Arc::clone(&self.storage),
            orchestrator: self.orchestrator.as_ref().map(Arc::clone),
            monitor: self.monitor.as_ref().map(Arc::clone),
            beeroom_realtime: self.beeroom_realtime.as_ref().map(Arc::clone),
            workspace: Arc::clone(&self.workspace),
            lsp_manager: Arc::clone(&self.lsp_manager),
            config: self.config,
            a2a_store: self.a2a_store,
            skills: self.skills,
            gateway: self.gateway.as_ref().map(Arc::clone),
            user_world: self.user_world.as_ref().map(Arc::clone),
            cron_wake_signal: self.cron_wake_signal.clone(),
            user_tool_manager: self.user_tool_manager.as_ref().map(Arc::clone),
            user_tool_bindings: self.user_tool_bindings,
            user_tool_store: self.user_tool_store,
            request_config_overrides: self.request_config_overrides,
            allow_roots: self.allow_roots.as_ref().map(Arc::clone),
            read_roots: self.read_roots.as_ref().map(Arc::clone),
            command_sessions: self.command_sessions.as_ref().map(Arc::clone),
            event_emitter,
            http: self.http,
        }
    }
}

#[derive(Clone)]
pub struct ToolRoots {
    pub allow_roots: Arc<Vec<PathBuf>>,
    pub read_roots: Arc<Vec<PathBuf>>,
}

pub(crate) fn is_allow_all_path_token(value: &str) -> bool {
    value.trim() == "*"
}

pub fn build_tool_roots(
    config: &Config,
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
    extra_roots: &[PathBuf],
) -> ToolRoots {
    let allow_roots = build_allow_roots(config);
    let mut allow_roots = allow_roots;
    allow_roots.extend(extra_roots.iter().cloned());
    let allow_roots = dedupe_roots(allow_roots);
    let mut read_roots = allow_roots.clone();
    read_roots.extend(build_skill_roots(skills, user_tool_bindings));
    read_roots.extend(extra_roots.iter().cloned());
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
        if is_allow_all_path_token(trimmed) {
            roots.extend(desktop_local_allow_roots());
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

pub(crate) fn roots_allow_any_path(roots: &[PathBuf]) -> bool {
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
    let allow_any_path = roots_allow_any_path(roots);
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        for root in roots {
            if is_within_root(root, &path) {
                return Some(path.clone());
            }
        }
        return None;
    }
    if allow_any_path {
        // When local desktop mode exposes filesystem roots, keep relative traversal available
        // so agent tools can follow user-provided local paths instead of failing on `..`.
        let cwd = std::env::current_dir().ok()?;
        let candidate = normalize_target_path(&cwd.join(path));
        for root in roots {
            if is_within_root(root, &candidate) {
                return Some(candidate.clone());
            }
        }
        return None;
    }
    let relative = sanitize_relative_path(trimmed)?;
    if let Some(resolved) = resolve_existing_round_relative_path(&relative, roots) {
        return Some(resolved);
    }
    for root in roots {
        let candidate = normalize_target_path(&root.join(&relative));
        if is_within_root(root, &candidate) {
            return Some(candidate);
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
        Ok(path) => {
            if !path.exists() && should_resolve_missing_path_from_extra_roots(raw_path) {
                if let Some(resolved) = resolve_path_in_roots(raw_path, extra_roots) {
                    return Ok(resolved);
                }
            }
            Ok(path)
        }
        Err(err) => {
            if let Some(resolved) = resolve_path_in_roots(raw_path, extra_roots) {
                Ok(resolved)
            } else {
                Err(err)
            }
        }
    }
}

fn resolve_existing_round_relative_path(relative: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let candidates = round_relative_path_candidates(relative)?;
    for root in roots {
        for candidate_relative in &candidates {
            let candidate = normalize_target_path(&root.join(candidate_relative));
            if is_within_root(root, &candidate) && candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

fn round_relative_path_candidates(relative: &Path) -> Option<Vec<PathBuf>> {
    let mut parts = relative.iter();
    let first = parts.next()?.to_string_lossy().to_string();
    let round_index = parse_round_index_token(&first)?;
    let remainder = parts.fold(PathBuf::new(), |mut acc, part| {
        acc.push(part);
        acc
    });
    let mut candidates = Vec::new();
    for alias in round_dir_aliases(round_index) {
        let mut candidate = PathBuf::from(alias);
        if !remainder.as_os_str().is_empty() {
            candidate.push(&remainder);
        }
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    Some(candidates)
}

fn should_resolve_missing_path_from_extra_roots(raw_path: &str) -> bool {
    let normalized = raw_path.trim().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    let Some(first) = stripped.split('/').next() else {
        return false;
    };
    parse_round_index_token(first).is_some()
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
    use super::{
        build_allow_roots, is_allow_all_path_token, resolve_path_in_roots, roots_allow_any_path,
        ToolEventEmitter,
    };
    use crate::config::Config;
    use crate::path_utils::normalize_existing_path;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
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
    fn wildcard_allow_path_expands_to_filesystem_roots() {
        let mut config = Config::default();
        config.security.allow_paths = vec!["*".to_string()];

        let roots = build_allow_roots(&config);

        assert!(is_allow_all_path_token("*"));
        assert!(roots_allow_any_path(&roots));
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

    #[test]
    fn resolve_path_in_roots_resolves_relative_path_against_each_root() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("private");
        let target = root.join("global").join("tooling.json");
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::write(&target, "{}").expect("write target");
        let roots = vec![root.clone()];

        let resolved = resolve_path_in_roots("global/tooling.json", &roots).expect("resolved");

        assert_eq!(
            normalize_existing_path(&resolved),
            normalize_existing_path(&target)
        );
    }

    #[test]
    fn resolve_tool_path_prefers_existing_extra_root_for_missing_workspace_path() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let run_root = workspace_root.join("orchestration").join("orch_demo");
        let target = run_root.join("round_0002").join("worker").join("report.txt");
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::write(&target, "ok").expect("write target");
        let db_path = temp.path().join("state.sqlite3");
        let storage = Arc::new(crate::storage::SqliteStorage::new(
            db_path.to_string_lossy().to_string(),
        ));
        let workspace = crate::workspace::WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &std::collections::HashMap::new(),
        );

        let resolved = super::resolve_tool_path(
            &workspace,
            "alice",
            "round_02/worker/report.txt",
            &[run_root],
        )
        .expect("resolved");

        assert_eq!(
            normalize_existing_path(&resolved),
            normalize_existing_path(&target)
        );
    }

    #[test]
    fn resolve_tool_path_keeps_regular_missing_workspace_path_in_workspace() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let extra_root = temp.path().join("extra");
        fs::create_dir_all(&extra_root).expect("mkdir extra");
        let storage = Arc::new(crate::storage::SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        let workspace = crate::workspace::WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &std::collections::HashMap::new(),
        );

        let resolved = super::resolve_tool_path(
            &workspace,
            "alice",
            "notes/new.txt",
            &[extra_root],
        )
        .expect("resolved");

        assert_eq!(resolved, workspace_root.join("alice").join("notes").join("new.txt"));
    }

    #[test]
    fn tool_event_emitter_merges_default_fields_into_object_payload() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let emitter = ToolEventEmitter::new(
            move |event_type, data| {
                sink.lock()
                    .expect("capture lock")
                    .push((event_type.to_string(), data));
            },
            true,
        )
        .with_field("tool_call_id", json!("call_demo"));

        emitter.emit(
            "tool_output_delta",
            json!({ "tool": "execute_command", "delta": "ok" }),
        );

        let entries = captured.lock().expect("capture lock");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "tool_output_delta");
        assert_eq!(entries[0].1["tool_call_id"], "call_demo");
    }

    #[test]
    fn tool_event_emitter_keeps_explicit_payload_fields() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&captured);
        let emitter = ToolEventEmitter::new(
            move |event_type, data| {
                sink.lock()
                    .expect("capture lock")
                    .push((event_type.to_string(), data));
            },
            true,
        )
        .with_field("tool_call_id", json!("call_default"));

        emitter.emit(
            "tool_output_delta",
            json!({ "tool": "execute_command", "tool_call_id": "call_explicit" }),
        );

        let entries = captured.lock().expect("capture lock");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1["tool_call_id"], "call_explicit");
    }

    #[test]
    fn tool_event_emitter_reads_default_string_field() {
        let emitter = ToolEventEmitter::new(|_, _| {}, true)
            .with_field("tool_call_id", json!("call_default"));
        assert_eq!(
            emitter.default_string_field("tool_call_id").as_deref(),
            Some("call_default")
        );
        assert!(emitter.default_string_field("missing").is_none());
    }
}
