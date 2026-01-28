// 工作区管理：路径校验、文件读写、目录操作与压缩打包。
use crate::i18n;
use crate::path_utils::is_within_root;
use crate::storage::StorageBackend;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Local};
use dashmap::DashMap;
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use tracing::warn;
use walkdir::WalkDir;

const TREE_CACHE_TTL_S: f64 = 5.0;
const TREE_CACHE_IDLE_TTL_S: f64 = 300.0;
const TREE_CACHE_MAX_USERS: usize = 512;
const SEARCH_INDEX_TTL_S: f64 = 10.0;
const SEARCH_INDEX_MAX_ITEMS: usize = 200_000;
const SEARCH_CACHE_IDLE_TTL_S: f64 = 300.0;
const SEARCH_CACHE_MAX_USERS: usize = 256;
const STORAGE_WRITE_QUEUE_SIZE: usize = 2048;
const TEMP_FILES_IDLE_TTL_S: f64 = 86_400.0;
const TEMP_FILES_CLEANUP_INTERVAL_S: f64 = 3600.0;
const SESSION_ACTIVITY_META_PREFIX: &str = "session_activity:";
const PUBLIC_WORKSPACE_ROOT: &str = "/workspaces";

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
    pub size: u64,
    pub updated_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<WorkspaceEntry>>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceTreeSnapshot {
    pub tree: String,
    pub version: u64,
}

#[derive(Debug, Clone)]
struct TreeCacheEntry {
    tree: String,
    built_ts: f64,
    last_access_ts: f64,
    version: u64,
}

#[derive(Default)]
struct TreeCache {
    cache: HashMap<String, TreeCacheEntry>,
    dirty: HashSet<String>,
}

#[derive(Default)]
struct RetentionState {
    last_cleanup: f64,
    running: bool,
}

#[derive(Default, Clone)]
struct UserUsageCache {
    data: HashMap<String, HashMap<String, i64>>,
    updated_ts: f64,
}

#[derive(Debug, Clone)]
struct SearchIndexEntry {
    entry: WorkspaceEntry,
    name_lower: String,
    is_dir: bool,
}

#[derive(Debug, Clone)]
struct SearchIndex {
    entries: Arc<Vec<SearchIndexEntry>>,
    built_ts: f64,
    last_access_ts: f64,
    version: u64,
}

enum StorageWrite {
    Chat { user_id: String, payload: Value },
    ToolLog { user_id: String, payload: Value },
    ArtifactLog { user_id: String, payload: Value },
}

struct StorageWriteQueue {
    sender: SyncSender<StorageWrite>,
    storage: Arc<dyn StorageBackend>,
}

impl StorageWriteQueue {
    fn new(storage: Arc<dyn StorageBackend>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(STORAGE_WRITE_QUEUE_SIZE);
        let worker_storage = storage.clone();
        thread::Builder::new()
            .name("wunder-storage-writer".to_string())
            .spawn(move || {
                while let Ok(task) = receiver.recv() {
                    if let Err(err) = Self::apply_write(&worker_storage, task) {
                        warn!("storage write failed: {err}");
                    }
                }
            })
            .ok();
        Self { sender, storage }
    }

    fn enqueue(&self, task: StorageWrite) -> Result<()> {
        match self.sender.try_send(task) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(task)) | Err(TrySendError::Disconnected(task)) => {
                Self::apply_write(&self.storage, task)
            }
        }
    }

    fn apply_write(storage: &Arc<dyn StorageBackend>, task: StorageWrite) -> Result<()> {
        match task {
            StorageWrite::Chat { user_id, payload } => storage.append_chat(&user_id, &payload),
            StorageWrite::ToolLog { user_id, payload } => {
                storage.append_tool_log(&user_id, &payload)
            }
            StorageWrite::ArtifactLog { user_id, payload } => {
                storage.append_artifact_log(&user_id, &payload)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PurgeResult {
    pub chat_records: i64,
    pub tool_records: i64,
    pub workspace_deleted: bool,
    pub legacy_history_deleted: bool,
}

pub struct WorkspaceManager {
    root: PathBuf,
    storage: Arc<dyn StorageBackend>,
    write_queue: StorageWriteQueue,
    retention_days: i64,
    retention_interval_s: f64,
    retention_state: Arc<Mutex<RetentionState>>,
    temp_cleanup_interval_s: f64,
    temp_cleanup_idle_ttl_s: f64,
    temp_cleanup_state: Arc<Mutex<RetentionState>>,
    versions: DashMap<String, u64>,
    path_guard: Option<Regex>,
    tree_cache: Mutex<TreeCache>,
    tree_cache_ttl_s: f64,
    tree_cache_idle_ttl_s: f64,
    tree_cache_max_users: usize,
    search_cache: Mutex<HashMap<String, SearchIndex>>,
    search_cache_ttl_s: f64,
    search_cache_max_items: usize,
    search_cache_idle_ttl_s: f64,
    search_cache_max_users: usize,
    user_usage_cache: Mutex<UserUsageCache>,
    user_usage_cache_ttl_s: f64,
}

impl WorkspaceManager {
    pub fn new(root: &str, storage: Arc<dyn StorageBackend>, retention_days: i64) -> Self {
        let retention_days = normalize_retention_days(retention_days);
        let _ = storage.ensure_initialized();
        let write_queue = StorageWriteQueue::new(storage.clone());
        Self {
            root: PathBuf::from(root),
            storage,
            write_queue,
            retention_days,
            retention_interval_s: 3600.0,
            retention_state: Arc::new(Mutex::new(RetentionState::default())),
            temp_cleanup_interval_s: TEMP_FILES_CLEANUP_INTERVAL_S,
            temp_cleanup_idle_ttl_s: TEMP_FILES_IDLE_TTL_S,
            temp_cleanup_state: Arc::new(Mutex::new(RetentionState::default())),
            versions: DashMap::new(),
            path_guard: match Regex::new(r#"[\\:*?\"<>|]"#) {
                Ok(regex) => Some(regex),
                Err(err) => {
                    warn!("invalid workspace path guard regex: {err}");
                    None
                }
            },
            tree_cache: Mutex::new(TreeCache::default()),
            tree_cache_ttl_s: TREE_CACHE_TTL_S,
            tree_cache_idle_ttl_s: TREE_CACHE_IDLE_TTL_S,
            tree_cache_max_users: TREE_CACHE_MAX_USERS,
            search_cache: Mutex::new(HashMap::new()),
            search_cache_ttl_s: SEARCH_INDEX_TTL_S,
            search_cache_max_items: SEARCH_INDEX_MAX_ITEMS,
            search_cache_idle_ttl_s: SEARCH_CACHE_IDLE_TTL_S,
            search_cache_max_users: SEARCH_CACHE_MAX_USERS,
            user_usage_cache: Mutex::new(UserUsageCache::default()),
            user_usage_cache_ttl_s: 5.0,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn workspace_root(&self, user_id: &str) -> PathBuf {
        let safe_id = self.safe_user_id(user_id);
        self.root.join(safe_id)
    }

    pub fn scoped_user_id(&self, user_id: &str, agent_id: Option<&str>) -> String {
        let safe_user = self.safe_user_id(user_id);
        let agent_id = agent_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty());
        let Some(agent_id) = agent_id else {
            return safe_user;
        };
        let safe_agent = self.safe_scope_component(agent_id);
        if safe_agent.is_empty() {
            safe_user
        } else {
            format!("{safe_user}__agent__{safe_agent}")
        }
    }

    fn safe_user_id(&self, user_id: &str) -> String {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return "anonymous".to_string();
        }
        let output = self.safe_scope_component(cleaned);
        if output.trim().is_empty() {
            "anonymous".to_string()
        } else {
            output
        }
    }

    fn safe_scope_component(&self, value: &str) -> String {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            return String::new();
        }
        let mut output = String::with_capacity(cleaned.len());
        for ch in cleaned.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                output.push(ch);
            } else {
                output.push('_');
            }
        }
        output
    }

    fn user_root(&self, user_id: &str) -> PathBuf {
        self.workspace_root(user_id)
    }

    pub fn public_root(&self, user_id: &str) -> PathBuf {
        let safe_id = self.safe_user_id(user_id);
        PathBuf::from(PUBLIC_WORKSPACE_ROOT).join(safe_id)
    }

    pub fn display_path(&self, user_id: &str, target: &Path) -> String {
        let user_root = self.user_root(user_id);
        if let Ok(rel) = target.strip_prefix(&user_root) {
            let public_root = self.public_root(user_id);
            let display = if rel.as_os_str().is_empty() {
                public_root
            } else {
                public_root.join(rel)
            };
            let mut text = display.to_string_lossy().replace('\\', "/");
            if rel.as_os_str().is_empty() && !text.ends_with('/') {
                text.push('/');
            }
            return text;
        }
        target.to_string_lossy().to_string()
    }

    pub fn map_public_path(&self, user_id: &str, target: &Path) -> Option<PathBuf> {
        let public_root = self.public_root(user_id);
        if !target.starts_with(&public_root) {
            return None;
        }
        let rel = target.strip_prefix(&public_root).ok()?;
        let user_root = self.user_root(user_id);
        if rel.as_os_str().is_empty() {
            Some(user_root)
        } else {
            Some(user_root.join(rel))
        }
    }

    pub fn replace_public_root_in_text(&self, user_id: &str, text: &str) -> String {
        let public_root = self
            .public_root(user_id)
            .to_string_lossy()
            .replace('\\', "/");
        if public_root.is_empty() {
            return text.to_string();
        }
        let user_root = self.user_root(user_id).to_string_lossy().replace('\\', "/");
        if public_root == user_root || !text.contains(&public_root) {
            return text.to_string();
        }
        text.replace(&public_root, &user_root)
    }

    fn session_context_tokens_key(&self, user_id: &str, session_id: &str) -> String {
        let safe_user = self.safe_user_id(user_id);
        let safe_session = session_id
            .trim()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let safe_session = if safe_session.trim().is_empty() {
            "default".to_string()
        } else {
            safe_session
        };
        format!("session_context_tokens:{safe_user}:{safe_session}")
    }

    fn maybe_schedule_retention_cleanup(&self) {
        if self.retention_days <= 0 {
            return;
        }
        let now = now_ts();
        {
            let mut state = self.retention_state.lock();
            if state.running || now - state.last_cleanup < self.retention_interval_s {
                return;
            }
            state.running = true;
            state.last_cleanup = now;
        }
        let storage = self.storage.clone();
        let retention_days = self.retention_days;
        let state = self.retention_state.clone();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                let _ =
                    tokio::task::spawn_blocking(move || storage.cleanup_retention(retention_days))
                        .await;
                let mut guard = state.lock();
                guard.running = false;
            });
        } else {
            let _ = storage.cleanup_retention(retention_days);
            let mut guard = state.lock();
            guard.running = false;
        }
    }

    fn maybe_schedule_temp_cleanup(&self) {
        if self.temp_cleanup_idle_ttl_s <= 0.0 {
            return;
        }
        let now = now_ts();
        {
            let mut state = self.temp_cleanup_state.lock();
            if state.running || now - state.last_cleanup < self.temp_cleanup_interval_s {
                return;
            }
            state.running = true;
            state.last_cleanup = now;
        }
        let root = self.root.clone();
        let storage = self.storage.clone();
        let idle_ttl_s = self.temp_cleanup_idle_ttl_s;
        let state = self.temp_cleanup_state.clone();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                let _ = tokio::task::spawn_blocking(move || {
                    cleanup_idle_temp_files(&root, &storage, idle_ttl_s);
                })
                .await;
                let mut guard = state.lock();
                guard.running = false;
            });
        } else {
            cleanup_idle_temp_files(&root, &storage, idle_ttl_s);
            let mut guard = state.lock();
            guard.running = false;
        }
    }

    pub fn resolve_path(&self, user_id: &str, path: &str) -> Result<PathBuf> {
        let trimmed = path.trim();
        let user_root = self.user_root(user_id);
        let target_path = Path::new(trimmed);
        if target_path.is_absolute() {
            if is_within_root(&user_root, target_path) {
                return Ok(target_path.to_path_buf());
            }
            if let Some(mapped) = self.map_public_path(user_id, target_path) {
                if is_within_root(&user_root, &mapped) {
                    return Ok(mapped);
                }
            }
            return Err(anyhow!("路径越界"));
        }
        if let Some(ref guard) = self.path_guard {
            if guard.is_match(trimmed) && !trimmed.is_empty() {
                return Err(anyhow!("路径包含非法字符"));
            }
        }
        for component in target_path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => {
                    return Err(anyhow!("路径不能为绝对路径"));
                }
                Component::ParentDir => {
                    return Err(anyhow!("路径不能包含 .."));
                }
                Component::CurDir => {}
                Component::Normal(_) => {}
            }
        }
        let target = if trimmed.is_empty() || trimmed == "." {
            user_root.clone()
        } else {
            user_root.join(trimmed)
        };
        if !is_within_root(&user_root, &target) {
            return Err(anyhow!("路径越界"));
        }
        Ok(target)
    }

    pub fn ensure_user_root(&self, user_id: &str) -> Result<PathBuf> {
        let user_root = self.user_root(user_id);
        fs::create_dir_all(&user_root)?;
        Ok(user_root)
    }

    pub fn touch_user_session(&self, user_id: &str) {
        let safe_id = self.safe_user_id(user_id);
        let key = session_activity_key(&safe_id);
        let now = now_ts();
        if let Err(err) = self.storage.set_meta(&key, &now.to_string()) {
            warn!("failed to record session activity for {safe_id}: {err}");
        }
        self.maybe_schedule_temp_cleanup();
    }

    pub fn list_workspace_entries(
        &self,
        user_id: &str,
        relative_path: &str,
        keyword: Option<&str>,
        offset: u64,
        limit: u64,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<WorkspaceEntry>, u64, String, Option<String>, u64)> {
        let normalized = normalize_relative_path(relative_path);
        let target = self.resolve_path(
            user_id,
            if normalized.is_empty() {
                "."
            } else {
                &normalized
            },
        )?;
        if !target.exists() {
            return Err(anyhow!(i18n::t("workspace.error.path_not_found")));
        }
        if !target.is_dir() {
            return Err(anyhow!(i18n::t("workspace.error.path_not_dir")));
        }

        let root = self.user_root(user_id);
        let keyword = keyword.unwrap_or("").trim().to_lowercase();
        let mut entries: Vec<(WorkspaceEntry, f64)> = Vec::new();
        for entry in fs::read_dir(&target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !keyword.is_empty() && !name.to_lowercase().contains(&keyword) {
                continue;
            }
            let meta = entry.metadata()?;
            let updated_ts = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0);
            let updated = meta.modified().ok().and_then(|time| {
                let dt: DateTime<Local> = time.into();
                Some(dt.to_rfc3339())
            });
            let entry_type = if meta.is_dir() { "dir" } else { "file" };
            let rel_path = entry
                .path()
                .strip_prefix(&root)
                .unwrap_or(entry.path().as_path())
                .to_string_lossy()
                .replace('\\', "/");
            let entry = WorkspaceEntry {
                name,
                path: rel_path,
                entry_type: entry_type.to_string(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                updated_time: updated.unwrap_or_default(),
                children: None,
            };
            entries.push((entry, updated_ts));
        }

        let total = entries.len() as u64;
        let sort_field = match sort_by {
            "size" | "updated_time" | "name" => sort_by,
            _ => "name",
        };
        let reverse = order.eq_ignore_ascii_case("desc");
        let sort_key = |payload: &(WorkspaceEntry, f64)| match sort_field {
            "size" => payload.0.size as f64,
            "updated_time" => payload.1,
            _ => 0.0,
        };
        let sort_name = |payload: &(WorkspaceEntry, f64)| payload.0.name.to_lowercase();

        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for payload in entries {
            if payload.0.entry_type == "dir" {
                dirs.push(payload);
            } else {
                files.push(payload);
            }
        }

        match sort_field {
            "name" => {
                dirs.sort_by(|a, b| sort_name(a).cmp(&sort_name(b)));
                files.sort_by(|a, b| sort_name(a).cmp(&sort_name(b)));
            }
            _ => {
                dirs.sort_by(|a, b| {
                    sort_key(a)
                        .partial_cmp(&sort_key(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                files.sort_by(|a, b| {
                    sort_key(a)
                        .partial_cmp(&sort_key(b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }
        if reverse {
            dirs.reverse();
            files.reverse();
        }
        let mut combined = Vec::with_capacity(dirs.len() + files.len());
        for (entry, _) in dirs.into_iter().chain(files.into_iter()) {
            combined.push(entry);
        }

        let safe_offset = offset as usize;
        let safe_limit = limit as usize;
        let sliced = if safe_offset == 0 && safe_limit == 0 {
            combined
        } else if safe_limit == 0 {
            combined.into_iter().skip(safe_offset).collect()
        } else {
            combined
                .into_iter()
                .skip(safe_offset)
                .take(safe_limit)
                .collect()
        };
        let parent = if normalized.is_empty() {
            None
        } else {
            let parent_path = Path::new(&normalized)
                .parent()
                .map(|path| path.to_string_lossy().to_string());
            match parent_path.as_deref() {
                Some("") | Some(".") | None => Some(String::new()),
                Some(value) => Some(value.to_string()),
            }
        };
        let tree_version = self.get_tree_version(user_id);
        Ok((sliced, tree_version, normalized, parent, total))
    }

    pub async fn list_workspace_entries_async(
        self: &Arc<Self>,
        user_id: &str,
        relative_path: &str,
        keyword: Option<&str>,
        offset: u64,
        limit: u64,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<WorkspaceEntry>, u64, String, Option<String>, u64)> {
        let user_id = user_id.to_string();
        let relative_path = relative_path.to_string();
        let keyword = keyword.map(|value| value.to_string());
        let sort_by = sort_by.to_string();
        let order = order.to_string();
        let workspace = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            workspace.list_workspace_entries(
                &user_id,
                &relative_path,
                keyword.as_deref(),
                offset,
                limit,
                &sort_by,
                &order,
            )
        })
        .await
        .map_err(|err| anyhow!("workspace list cancelled: {err}"))?
    }

    pub async fn load_history_async(
        self: &Arc<Self>,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let workspace = Arc::clone(self);
        tokio::task::spawn_blocking(move || workspace.load_history(&user_id, &session_id, limit))
            .await
            .map_err(|err| anyhow!("workspace load history cancelled: {err}"))?
    }

    pub async fn load_session_system_prompt_async(
        self: &Arc<Self>,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>> {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let language = language.map(|value| value.to_string());
        let workspace = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            workspace.load_session_system_prompt(&user_id, &session_id, language.as_deref())
        })
        .await
        .map_err(|err| anyhow!("workspace load session prompt cancelled: {err}"))?
    }

    pub async fn load_session_context_tokens_async(
        self: &Arc<Self>,
        user_id: &str,
        session_id: &str,
    ) -> i64 {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let workspace = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            workspace.load_session_context_tokens(&user_id, &session_id)
        })
        .await
        .unwrap_or(0)
    }

    pub async fn save_session_context_tokens_async(
        self: &Arc<Self>,
        user_id: &str,
        session_id: &str,
        total_tokens: i64,
    ) {
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        let workspace = Arc::clone(self);
        let _ = tokio::task::spawn_blocking(move || {
            workspace.save_session_context_tokens(&user_id, &session_id, total_tokens);
        })
        .await;
    }

    pub fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.write_queue.enqueue(StorageWrite::Chat {
            user_id: user_id.to_string(),
            payload: payload.clone(),
        })?;
        self.maybe_schedule_retention_cleanup();
        Ok(())
    }

    pub fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.write_queue.enqueue(StorageWrite::ToolLog {
            user_id: user_id.to_string(),
            payload: payload.clone(),
        })?;
        self.maybe_schedule_retention_cleanup();
        Ok(())
    }

    pub fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.write_queue.enqueue(StorageWrite::ArtifactLog {
            user_id: user_id.to_string(),
            payload: payload.clone(),
        })?;
        self.maybe_schedule_retention_cleanup();
        Ok(())
    }

    pub fn load_history(&self, user_id: &str, session_id: &str, limit: i64) -> Result<Vec<Value>> {
        let limit = normalize_history_limit(limit);
        self.storage.load_chat_history(user_id, session_id, limit)
    }

    pub fn load_artifact_logs(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<Value>> {
        self.storage.load_artifact_logs(user_id, session_id, limit)
    }

    pub fn load_stream_events(
        &self,
        session_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Vec<Value> {
        self.storage
            .load_stream_events(session_id, after_event_id, limit)
            .unwrap_or_default()
    }

    pub fn load_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>> {
        self.storage
            .get_session_system_prompt(user_id, session_id, language)
    }

    pub fn load_session_context_tokens(&self, user_id: &str, session_id: &str) -> i64 {
        let key = self.session_context_tokens_key(user_id, session_id);
        let Ok(value) = self.storage.get_meta(&key) else {
            return 0;
        };
        value
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .unwrap_or(0)
    }

    pub fn save_session_context_tokens(&self, user_id: &str, session_id: &str, total_tokens: i64) {
        let key = self.session_context_tokens_key(user_id, session_id);
        let value = total_tokens.max(0).to_string();
        let _ = self.storage.set_meta(&key, &value);
    }

    pub fn delete_session_context_tokens(&self, user_id: &str, session_id: &str) -> i64 {
        let key = self.session_context_tokens_key(user_id, session_id);
        self.storage.delete_meta_prefix(&key).unwrap_or(0) as i64
    }

    pub fn save_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        prompt: &str,
        language: Option<&str>,
    ) -> Result<()> {
        let content = prompt.trim();
        if content.is_empty() {
            return Ok(());
        }
        let payload = serde_json::json!({
            "role": "system",
            "content": content,
            "session_id": session_id,
            "timestamp": Local::now().to_rfc3339(),
            "meta": {
                "type": "system_prompt",
                "language": language.unwrap_or("").trim(),
            }
        });
        self.append_chat(user_id, &payload)
    }

    pub fn get_user_usage_stats(&self) -> HashMap<String, HashMap<String, i64>> {
        let now = now_ts();
        {
            let cache = self.user_usage_cache.lock();
            if cache.updated_ts > 0.0 && now - cache.updated_ts < self.user_usage_cache_ttl_s {
                return cache.data.clone();
            }
        }
        let chat_stats = self.storage.get_user_chat_stats().unwrap_or_default();
        let tool_stats = self.storage.get_user_tool_stats().unwrap_or_default();
        let mut combined: HashMap<String, HashMap<String, i64>> = HashMap::new();
        for (user_id, stats) in chat_stats {
            let mut entry = HashMap::new();
            entry.insert(
                "chat_records".to_string(),
                *stats.get("chat_records").unwrap_or(&0),
            );
            entry.insert("tool_records".to_string(), 0);
            combined.insert(user_id, entry);
        }
        for (user_id, stats) in tool_stats {
            let entry = combined.entry(user_id).or_insert_with(|| {
                let mut entry = HashMap::new();
                entry.insert("chat_records".to_string(), 0);
                entry.insert("tool_records".to_string(), 0);
                entry
            });
            let count = *stats.get("tool_records").unwrap_or(&0);
            entry.insert("tool_records".to_string(), count);
        }
        let mut cache = self.user_usage_cache.lock();
        cache.data = combined.clone();
        cache.updated_ts = now;
        combined
    }

    pub fn get_tool_usage_stats(
        &self,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Vec<HashMap<String, Value>> {
        let stats = self
            .storage
            .get_tool_usage_stats(since_time, until_time)
            .unwrap_or_default();
        stats
            .into_iter()
            .map(|(tool, calls)| {
                let mut entry = HashMap::new();
                entry.insert("tool".to_string(), json!(tool));
                entry.insert("calls".to_string(), json!(calls));
                entry
            })
            .collect()
    }

    pub fn get_tool_session_usage(
        &self,
        tool: &str,
        since_time: Option<f64>,
        until_time: Option<f64>,
    ) -> Vec<HashMap<String, Value>> {
        self.storage
            .get_tool_session_usage(tool, since_time, until_time)
            .unwrap_or_default()
    }

    pub fn purge_session_data(&self, user_id: &str, session_id: &str) {
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return;
        }
        let _ = self
            .storage
            .delete_chat_history_by_session(cleaned_user, cleaned_session);
        let _ = self
            .storage
            .delete_tool_logs_by_session(cleaned_user, cleaned_session);
        let _ = self
            .storage
            .delete_artifact_logs_by_session(cleaned_user, cleaned_session);
        let _ = self
            .storage
            .delete_stream_events_by_session(cleaned_session);
        let _ = self.storage.release_session_lock(cleaned_session);
        let _ = self.delete_session_context_tokens(cleaned_user, cleaned_session);
    }

    pub fn purge_user_data(&self, user_id: &str) -> PurgeResult {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return PurgeResult {
                chat_records: 0,
                tool_records: 0,
                workspace_deleted: false,
                legacy_history_deleted: false,
            };
        }
        let chat_deleted = self.storage.delete_chat_history(cleaned).unwrap_or(0);
        let tool_deleted = self.storage.delete_tool_logs(cleaned).unwrap_or(0);
        let _ = self.storage.delete_memory_records_by_user(cleaned);
        let _ = self.storage.delete_memory_settings_by_user(cleaned);
        let _ = self.storage.delete_artifact_logs(cleaned);
        let workspace_root = self.workspace_root(cleaned);
        let workspace_deleted = fs::remove_dir_all(&workspace_root).is_ok();
        let legacy_history_deleted = false;
        let safe_id = self.safe_user_id(cleaned);
        {
            let mut cache = self.tree_cache.lock();
            cache.cache.remove(&safe_id);
            cache.dirty.remove(&safe_id);
        }
        {
            let mut cache = self.search_cache.lock();
            cache.remove(&safe_id);
        }
        let _ = self.versions.remove(&safe_id);
        {
            let mut cache = self.user_usage_cache.lock();
            cache.data.remove(cleaned);
            cache.updated_ts = 0.0;
        }
        let _ = self
            .storage
            .delete_meta_prefix(&format!("session_context_tokens:{safe_id}:"));
        let _ = self.storage.delete_session_locks_by_user(cleaned);
        let _ = self.storage.delete_stream_events_by_user(cleaned);
        PurgeResult {
            chat_records: chat_deleted,
            tool_records: tool_deleted,
            workspace_deleted,
            legacy_history_deleted,
        }
    }

    pub fn write_file(
        &self,
        user_id: &str,
        path: &str,
        content: &str,
        create_if_missing: bool,
    ) -> Result<()> {
        let target = self.resolve_path(user_id, path)?;
        if !create_if_missing && !target.exists() {
            return Err(anyhow!("文件不存在"));
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&target)?;
        file.write_all(content.as_bytes())?;
        self.bump_version(user_id);
        Ok(())
    }

    pub fn search_workspace_entries(
        &self,
        user_id: &str,
        keyword: &str,
        offset: u64,
        limit: u64,
        include_files: bool,
        include_dirs: bool,
    ) -> Result<(Vec<WorkspaceEntry>, u64)> {
        let keyword = keyword.trim().to_lowercase();
        if keyword.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let root = self.ensure_user_root(user_id)?;
        let safe_id = self.safe_user_id(user_id);
        let safe_offset = offset.max(0);
        let safe_limit = limit.max(0);
        let version = self.get_tree_version(user_id);
        let now = now_ts();
        if let Some(index) = self.get_search_index(&safe_id, version, now) {
            return Ok(search_from_index(
                &index,
                &keyword,
                safe_offset,
                safe_limit,
                include_files,
                include_dirs,
            ));
        }
        if let Some(index) = self.build_search_index(&root, version) {
            let result = search_from_index(
                &index,
                &keyword,
                safe_offset,
                safe_limit,
                include_files,
                include_dirs,
            );
            self.store_search_index(safe_id, index);
            return Ok(result);
        }
        Ok(search_by_walkdir(
            &root,
            &keyword,
            safe_offset,
            safe_limit,
            include_files,
            include_dirs,
        ))
    }

    pub async fn search_workspace_entries_async(
        self: &Arc<Self>,
        user_id: &str,
        keyword: &str,
        offset: u64,
        limit: u64,
        include_files: bool,
        include_dirs: bool,
    ) -> Result<(Vec<WorkspaceEntry>, u64)> {
        let user_id = user_id.to_string();
        let keyword = keyword.to_string();
        let workspace = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            workspace.search_workspace_entries(
                &user_id,
                &keyword,
                offset,
                limit,
                include_files,
                include_dirs,
            )
        })
        .await
        .map_err(|err| anyhow!("workspace search cancelled: {err}"))?
    }

    pub fn get_workspace_tree_snapshot(&self, user_id: &str) -> WorkspaceTreeSnapshot {
        let safe_id = self.safe_user_id(user_id);
        let now = now_ts();
        {
            let mut cache = self.tree_cache.lock();
            let dirty = cache.dirty.contains(&safe_id);
            if let Some(entry) = cache.cache.get_mut(&safe_id) {
                entry.last_access_ts = now;
                let stale = now - entry.built_ts >= self.tree_cache_ttl_s;
                if !dirty || !stale {
                    let snapshot = WorkspaceTreeSnapshot {
                        tree: entry.tree.clone(),
                        version: entry.version,
                    };
                    self.evict_tree_cache_locked(&mut cache, now);
                    return snapshot;
                }
            }
            self.evict_tree_cache_locked(&mut cache, now);
        }
        let tree = self.refresh_workspace_tree(user_id);
        WorkspaceTreeSnapshot {
            tree,
            version: self.get_tree_cache_version(user_id),
        }
    }

    pub fn refresh_workspace_tree(&self, user_id: &str) -> String {
        let root = match self.ensure_user_root(user_id) {
            Ok(path) => path,
            Err(_) => return i18n::t("workspace.tree.empty"),
        };
        let tree = if Handle::try_current().is_ok() {
            tokio::task::block_in_place(|| build_workspace_tree(&root, 2))
        } else {
            build_workspace_tree(&root, 2)
        };
        let now = now_ts();
        let safe_id = self.safe_user_id(user_id);
        let mut cache = self.tree_cache.lock();
        let was_dirty = cache.dirty.remove(&safe_id);
        let entry = cache
            .cache
            .entry(safe_id.clone())
            .or_insert(TreeCacheEntry {
                tree: String::new(),
                built_ts: 0.0,
                last_access_ts: now,
                version: 0,
            });
        let changed = entry.tree != tree;
        if changed {
            entry.version = entry.version.saturating_add(1).max(1);
            if !was_dirty {
                self.increment_version(&safe_id);
            }
        }
        entry.tree = tree.clone();
        entry.built_ts = now;
        entry.last_access_ts = now;
        self.evict_tree_cache_locked(&mut cache, now);
        tree
    }

    pub fn mark_tree_dirty(&self, user_id: &str) {
        let safe_id = self.safe_user_id(user_id);
        self.increment_version(&safe_id);
        let mut cache = self.tree_cache.lock();
        cache.dirty.insert(safe_id);
    }

    pub fn get_tree_version(&self, user_id: &str) -> u64 {
        let safe_id = self.safe_user_id(user_id);
        self.versions.get(&safe_id).map(|value| *value).unwrap_or(0)
    }

    pub fn get_tree_cache_version(&self, user_id: &str) -> u64 {
        let safe_id = self.safe_user_id(user_id);
        let cache = self.tree_cache.lock();
        cache
            .cache
            .get(&safe_id)
            .map(|entry| entry.version)
            .unwrap_or(0)
    }

    pub fn bump_version(&self, user_id: &str) {
        self.mark_tree_dirty(user_id);
    }

    fn increment_version(&self, safe_id: &str) {
        let mut entry = self.versions.entry(safe_id.to_string()).or_insert(0);
        *entry += 1;
    }

    fn evict_tree_cache_locked(&self, cache: &mut TreeCache, now: f64) {
        let idle_ttl = self.tree_cache_idle_ttl_s;
        if idle_ttl > 0.0 {
            let cutoff = now - idle_ttl;
            let mut stale_keys = Vec::new();
            for (key, entry) in cache.cache.iter() {
                let last_access = if entry.last_access_ts > 0.0 {
                    entry.last_access_ts
                } else {
                    entry.built_ts
                };
                if last_access > 0.0 && last_access < cutoff {
                    stale_keys.push(key.clone());
                }
            }
            for key in stale_keys {
                cache.cache.remove(&key);
                cache.dirty.remove(&key);
            }
        }

        let max_entries = self.tree_cache_max_users;
        if max_entries > 0 && cache.cache.len() > max_entries {
            let mut items = cache
                .cache
                .iter()
                .map(|(key, entry)| {
                    let last_access = if entry.last_access_ts > 0.0 {
                        entry.last_access_ts
                    } else {
                        entry.built_ts
                    };
                    (key.clone(), last_access)
                })
                .collect::<Vec<_>>();
            items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
            let overflow = cache.cache.len().saturating_sub(max_entries);
            for (key, _) in items.into_iter().take(overflow) {
                cache.cache.remove(&key);
                cache.dirty.remove(&key);
            }
        }
    }

    fn evict_search_cache_locked(&self, cache: &mut HashMap<String, SearchIndex>, now: f64) {
        let idle_ttl = self.search_cache_idle_ttl_s;
        if idle_ttl > 0.0 {
            let cutoff = now - idle_ttl;
            let mut stale_keys = Vec::new();
            for (key, entry) in cache.iter() {
                let last_access = if entry.last_access_ts > 0.0 {
                    entry.last_access_ts
                } else {
                    entry.built_ts
                };
                if last_access > 0.0 && last_access < cutoff {
                    stale_keys.push(key.clone());
                }
            }
            for key in stale_keys {
                cache.remove(&key);
            }
        }

        let max_entries = self.search_cache_max_users;
        if max_entries > 0 && cache.len() > max_entries {
            let mut items = cache
                .iter()
                .map(|(key, entry)| {
                    let last_access = if entry.last_access_ts > 0.0 {
                        entry.last_access_ts
                    } else {
                        entry.built_ts
                    };
                    (key.clone(), last_access)
                })
                .collect::<Vec<_>>();
            items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
            let overflow = cache.len().saturating_sub(max_entries);
            for (key, _) in items.into_iter().take(overflow) {
                cache.remove(&key);
            }
        }
    }

    fn get_search_index(&self, safe_id: &str, version: u64, now: f64) -> Option<SearchIndex> {
        let mut cache = self.search_cache.lock();
        let Some(entry) = cache.get_mut(safe_id) else {
            return None;
        };
        if entry.version != version {
            cache.remove(safe_id);
            self.evict_search_cache_locked(&mut cache, now);
            return None;
        }
        if now - entry.built_ts >= self.search_cache_ttl_s {
            cache.remove(safe_id);
            self.evict_search_cache_locked(&mut cache, now);
            return None;
        }
        entry.last_access_ts = now;
        let cloned = entry.clone();
        self.evict_search_cache_locked(&mut cache, now);
        Some(cloned)
    }

    fn store_search_index(&self, safe_id: String, index: SearchIndex) {
        let mut cache = self.search_cache.lock();
        cache.insert(safe_id, index);
        self.evict_search_cache_locked(&mut cache, now_ts());
    }

    fn build_search_index(&self, root: &Path, version: u64) -> Option<SearchIndex> {
        let mut entries = Vec::new();
        for entry in WalkDir::new(root)
            .min_depth(1)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            if entries.len() >= self.search_cache_max_items {
                return None;
            }
            let file_type = entry.file_type();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            let meta = entry.metadata().ok();
            let updated = meta
                .as_ref()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| {
                    let dt: DateTime<Local> = time.into();
                    Some(dt.to_rfc3339())
                });
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            let is_dir = file_type.is_dir();
            let entry = WorkspaceEntry {
                name: name.clone(),
                path: rel.replace('\\', "/"),
                entry_type: if is_dir { "dir" } else { "file" }.to_string(),
                size: meta.map(|meta| meta.len()).unwrap_or(0),
                updated_time: updated.unwrap_or_default(),
                children: None,
            };
            entries.push(SearchIndexEntry {
                entry,
                name_lower: name.to_lowercase(),
                is_dir,
            });
        }
        let now = now_ts();
        Some(SearchIndex {
            entries: Arc::new(entries),
            built_ts: now,
            last_access_ts: now,
            version,
        })
    }
}

fn normalize_relative_path(value: &str) -> String {
    let trimmed = value.replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').to_string()
}

fn session_activity_key(safe_id: &str) -> String {
    format!("{SESSION_ACTIVITY_META_PREFIX}{safe_id}")
}

fn parse_session_activity_ts(value: Option<String>) -> Option<f64> {
    value.and_then(|value| value.parse::<f64>().ok())
}

fn dir_modified_ts(path: &Path) -> Option<f64> {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
}

fn cleanup_idle_temp_files(root: &Path, storage: &Arc<dyn StorageBackend>, idle_ttl_s: f64) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let now = now_ts();
    for entry in entries.flatten() {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_dir() {
            continue;
        }
        let safe_id = entry.file_name().to_string_lossy().to_string();
        if safe_id.is_empty() {
            continue;
        }
        let workspace_root = entry.path();
        if !workspace_root.exists() {
            continue;
        }
        let last_seen = parse_session_activity_ts(
            storage
                .get_meta(&session_activity_key(&safe_id))
                .ok()
                .flatten(),
        )
        .or_else(|| dir_modified_ts(&workspace_root));
        let Some(last_seen) = last_seen else {
            continue;
        };
        if now - last_seen < idle_ttl_s {
            continue;
        }
        clear_dir_contents(&workspace_root);
    }
}

fn clear_dir_contents(path: &Path) {
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) => {
            warn!("failed to read temp dir {}: {err}", path.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let target = entry.path();
        let result = match entry.file_type() {
            Ok(file_type) if file_type.is_dir() => fs::remove_dir_all(&target),
            Ok(_) => fs::remove_file(&target),
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            warn!("failed to remove temp entry {}: {err}", target.display());
        }
    }
}

fn build_workspace_tree(root: &Path, max_depth: usize) -> String {
    if !root.exists() {
        return i18n::t("workspace.tree.empty");
    }
    let mut lines = Vec::new();
    build_workspace_tree_inner(root, 0, max_depth, &mut lines);
    if lines.is_empty() {
        i18n::t("workspace.tree.empty")
    } else {
        lines.join("\n")
    }
}

fn build_workspace_tree_inner(
    path: &Path,
    depth: usize,
    max_depth: usize,
    lines: &mut Vec<String>,
) {
    if depth > max_depth {
        return;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.is_dir() {
            dirs.push((name, entry.path()));
        } else {
            files.push(name);
        }
    }
    dirs.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    files.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    let prefix = "  ".repeat(depth);
    for (name, dir_path) in dirs {
        lines.push(format!("{prefix}{name}/"));
        if depth < max_depth {
            build_workspace_tree_inner(&dir_path, depth + 1, max_depth, lines);
        }
    }
    for name in files {
        lines.push(format!("{prefix}{name}"));
    }
}

fn search_from_index(
    index: &SearchIndex,
    keyword: &str,
    offset: u64,
    limit: u64,
    include_files: bool,
    include_dirs: bool,
) -> (Vec<WorkspaceEntry>, u64) {
    let mut matched = 0u64;
    let mut results = Vec::new();
    for item in index.entries.iter() {
        if item.is_dir && !include_dirs {
            continue;
        }
        if !item.is_dir && !include_files {
            continue;
        }
        if !item.name_lower.contains(keyword) {
            continue;
        }
        matched += 1;
        if matched <= offset {
            continue;
        }
        if limit == 0 || results.len() < limit as usize {
            results.push(item.entry.clone());
        }
    }
    (results, matched)
}

fn search_by_walkdir(
    root: &Path,
    keyword: &str,
    offset: u64,
    limit: u64,
    include_files: bool,
    include_dirs: bool,
) -> (Vec<WorkspaceEntry>, u64) {
    let mut matched = 0u64;
    let mut results = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let file_type = entry.file_type();
        if file_type.is_dir() && !include_dirs {
            continue;
        }
        if file_type.is_file() && !include_files {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.to_lowercase().contains(keyword) {
            continue;
        }
        matched += 1;
        if matched <= offset {
            continue;
        }
        let meta = entry.metadata().ok();
        let updated = meta
            .as_ref()
            .and_then(|meta| meta.modified().ok())
            .and_then(|time| {
                let dt: DateTime<Local> = time.into();
                Some(dt.to_rfc3339())
            });
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .to_string();
        if limit == 0 || results.len() < limit as usize {
            results.push(WorkspaceEntry {
                name,
                path: rel.replace('\\', "/"),
                entry_type: if file_type.is_dir() { "dir" } else { "file" }.to_string(),
                size: meta.map(|meta| meta.len()).unwrap_or(0),
                updated_time: updated.unwrap_or_default(),
                children: None,
            });
        }
    }
    (results, matched)
}

fn normalize_history_limit(limit: i64) -> Option<i64> {
    if limit <= 0 {
        None
    } else {
        Some(limit)
    }
}

fn normalize_retention_days(value: i64) -> i64 {
    if value <= 0 {
        0
    } else {
        value
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
