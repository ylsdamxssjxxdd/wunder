// 工作区管理：路径校验、文件读写、目录操作与压缩打包。
use crate::i18n;
use crate::path_utils::is_within_root;
use crate::storage::StorageBackend;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, Utc};
use dashmap::DashMap;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Handle;
use walkdir::WalkDir;
use zip::write::FileOptions;

const MIGRATION_MARKER: &str = ".wunder_workspace_v2";
const LEGACY_CHAT_FILE: &str = "chat_history.jsonl";
const LEGACY_TOOL_FILE: &str = "tool_log.jsonl";

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

#[derive(Default)]
struct TreeCache {
    cache: HashMap<String, String>,
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
pub struct PurgeResult {
    pub chat_records: i64,
    pub tool_records: i64,
    pub workspace_deleted: bool,
    pub legacy_history_deleted: bool,
}

pub struct WorkspaceManager {
    root: PathBuf,
    history_root: PathBuf,
    storage: Arc<dyn StorageBackend>,
    retention_days: i64,
    retention_interval_s: f64,
    retention_state: Arc<Mutex<RetentionState>>,
    versions: DashMap<String, u64>,
    path_guard: Regex,
    tree_cache: Mutex<TreeCache>,
    user_usage_cache: Mutex<UserUsageCache>,
    user_usage_cache_ttl_s: f64,
}

impl WorkspaceManager {
    pub fn new(root: &str, storage: Arc<dyn StorageBackend>, retention_days: i64) -> Self {
        let retention_days = normalize_retention_days(retention_days);
        let history_root = PathBuf::from("data/historys");
        let _ = storage.ensure_initialized();
        Self {
            root: PathBuf::from(root),
            history_root,
            storage,
            retention_days,
            retention_interval_s: 3600.0,
            retention_state: Arc::new(Mutex::new(RetentionState::default())),
            versions: DashMap::new(),
            path_guard: Regex::new(r#"[\\:*?\"<>|]"#).unwrap(),
            tree_cache: Mutex::new(TreeCache::default()),
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

    fn safe_user_id(&self, user_id: &str) -> String {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return "anonymous".to_string();
        }
        let mut output = String::with_capacity(cleaned.len());
        for ch in cleaned.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                output.push(ch);
            } else {
                output.push('_');
            }
        }
        if output.trim().is_empty() {
            "anonymous".to_string()
        } else {
            output
        }
    }

    fn user_root(&self, user_id: &str) -> PathBuf {
        self.workspace_root(user_id).join("files")
    }

    fn history_root(&self, user_id: &str) -> PathBuf {
        let safe_id = self.safe_user_id(user_id);
        self.history_root.join(safe_id)
    }

    fn history_migration_key(&self, user_id: &str) -> String {
        format!("history_migrated:{}", self.safe_user_id(user_id))
    }

    fn session_token_usage_key(&self, user_id: &str, session_id: &str) -> String {
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
        format!("session_token_usage:{safe_user}:{safe_session}")
    }

    fn ensure_history_storage(&self, _user_id: &str) {
        let _ = self.storage.ensure_initialized();
    }

    fn migrate_legacy_history(&self, user_id: &str, workspace_root: &Path) {
        let migration_key = self.history_migration_key(user_id);
        if self
            .storage
            .get_meta(&migration_key)
            .ok()
            .flatten()
            .as_deref()
            == Some("1")
        {
            return;
        }
        let mut migrated = false;
        let history_root = self.history_root(user_id);
        let mut legacy_roots = vec![workspace_root.to_path_buf()];
        if history_root.exists() {
            legacy_roots.push(history_root.clone());
        }
        let mut seen_paths = HashSet::new();
        for (filename, kind) in [(LEGACY_CHAT_FILE, "chat"), (LEGACY_TOOL_FILE, "tool")] {
            for legacy_root in &legacy_roots {
                let legacy_path = legacy_root.join(filename);
                if !seen_paths.insert(legacy_path.clone()) {
                    continue;
                }
                if !legacy_path.exists() {
                    continue;
                }
                for payload in read_jsonl(&legacy_path) {
                    let _ = match kind {
                        "chat" => self.storage.append_chat(user_id, &payload),
                        "tool" => self.storage.append_tool_log(user_id, &payload),
                        _ => Ok(()),
                    };
                    migrated = true;
                }
                if legacy_root == workspace_root {
                    let target_root = self.history_root(user_id);
                    let _ = fs::create_dir_all(&target_root);
                    let mut target = target_root.join(filename);
                    if target.exists() {
                        target = resolve_collision(&target);
                    }
                    if fs::rename(&legacy_path, &target).is_err() {
                        if fs::copy(&legacy_path, &target).is_ok() {
                            let _ = fs::remove_file(&legacy_path);
                        }
                    }
                }
            }
        }
        if migrated {
            let _ = self.storage.set_meta(&migration_key, "1");
        }
    }

    fn migrate_legacy_files(&self, _user_id: &str, workspace_root: &Path, files_root: &Path) {
        let marker = workspace_root.join(MIGRATION_MARKER);
        if marker.exists() {
            return;
        }
        let reserved: HashSet<&str> = HashSet::from([
            "files",
            LEGACY_CHAT_FILE,
            LEGACY_TOOL_FILE,
            MIGRATION_MARKER,
        ]);
        let entries = match fs::read_dir(workspace_root) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if reserved.contains(name.as_str()) {
                continue;
            }
            let target = files_root.join(&name);
            let target = if target.exists() {
                resolve_collision(&target)
            } else {
                target
            };
            let _ = fs::rename(entry.path(), target);
        }
        let _ = fs::write(marker, "");
    }

    fn maybe_schedule_retention_cleanup(&self) {
        if self.retention_days <= 0 {
            return;
        }
        let now = now_ts();
        {
            let mut state = self.retention_state.lock().unwrap();
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
                let mut guard = state.lock().unwrap();
                guard.running = false;
            });
        } else {
            let _ = storage.cleanup_retention(retention_days);
            let mut guard = state.lock().unwrap();
            guard.running = false;
        }
    }

    pub fn resolve_path(&self, user_id: &str, path: &str) -> Result<PathBuf> {
        if self.path_guard.is_match(path) && !path.is_empty() {
            return Err(anyhow!("路径包含非法字符"));
        }
        let target_path = Path::new(path);
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
        let user_root = self.user_root(user_id);
        let target = if path.is_empty() || path == "." {
            user_root.clone()
        } else {
            user_root.join(path)
        };
        if !is_within_root(&user_root, &target) {
            return Err(anyhow!("路径越界"));
        }
        Ok(target)
    }

    pub fn ensure_user_root(&self, user_id: &str) -> Result<PathBuf> {
        let workspace_root = self.workspace_root(user_id);
        let user_root = self.user_root(user_id);
        fs::create_dir_all(&user_root)?;
        self.ensure_history_storage(user_id);
        self.migrate_legacy_history(user_id, &workspace_root);
        self.migrate_legacy_files(user_id, &workspace_root, &user_root);
        Ok(user_root)
    }

    pub fn list_entries(&self, user_id: &str, path: &str) -> Result<Vec<WorkspaceEntry>> {
        let target = self.resolve_path(user_id, path)?;
        let mut entries = Vec::new();
        if !target.exists() {
            return Ok(entries);
        }
        for entry in fs::read_dir(&target)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            let entry_type = if meta.is_dir() { "dir" } else { "file" };
            let updated = meta.modified().ok().and_then(|time| {
                let dt: DateTime<Local> = time.into();
                Some(dt.to_rfc3339())
            });
            let name = entry.file_name().to_string_lossy().to_string();
            let rel_path = Path::new(path).join(&name);
            entries.push(WorkspaceEntry {
                name,
                path: rel_path.to_string_lossy().to_string(),
                entry_type: entry_type.to_string(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                updated_time: updated.unwrap_or_default(),
                children: None,
            });
        }
        Ok(entries)
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

    pub fn append_chat(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.storage.append_chat(user_id, payload)?;
        self.maybe_schedule_retention_cleanup();
        Ok(())
    }

    pub fn append_tool_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.storage.append_tool_log(user_id, payload)?;
        self.maybe_schedule_retention_cleanup();
        Ok(())
    }

    pub fn append_artifact_log(&self, user_id: &str, payload: &Value) -> Result<()> {
        self.storage.append_artifact_log(user_id, payload)?;
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

    pub fn load_session_system_prompt(
        &self,
        user_id: &str,
        session_id: &str,
        language: Option<&str>,
    ) -> Result<Option<String>> {
        self.storage
            .get_session_system_prompt(user_id, session_id, language)
    }

    pub fn load_session_token_usage(&self, user_id: &str, session_id: &str) -> i64 {
        let key = self.session_token_usage_key(user_id, session_id);
        let Ok(value) = self.storage.get_meta(&key) else {
            return 0;
        };
        value
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .unwrap_or(0)
    }

    pub fn save_session_token_usage(&self, user_id: &str, session_id: &str, total_tokens: i64) {
        let key = self.session_token_usage_key(user_id, session_id);
        let value = total_tokens.max(0).to_string();
        let _ = self.storage.set_meta(&key, &value);
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
            "timestamp": Utc::now().to_rfc3339(),
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
            let cache = self.user_usage_cache.lock().unwrap();
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
        let mut cache = self.user_usage_cache.lock().unwrap();
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
        let legacy_root = self.history_root(cleaned);
        let legacy_history_deleted = fs::remove_dir_all(&legacy_root).is_ok();
        let safe_id = self.safe_user_id(cleaned);
        {
            let mut cache = self.tree_cache.lock().unwrap();
            cache.cache.remove(&safe_id);
            cache.dirty.remove(&safe_id);
        }
        let _ = self.versions.remove(&safe_id);
        let _ = self
            .storage
            .delete_meta_prefix(&format!("session_token_usage:{safe_id}:"));
        let _ = self.storage.delete_session_locks_by_user(cleaned);
        let _ = self.storage.delete_stream_events_by_user(cleaned);
        PurgeResult {
            chat_records: chat_deleted,
            tool_records: tool_deleted,
            workspace_deleted,
            legacy_history_deleted,
        }
    }

    pub fn read_file(&self, user_id: &str, path: &str, max_bytes: usize) -> Result<String> {
        let target = self.resolve_path(user_id, path)?;
        let file = fs::File::open(&target)?;
        let mut buffer = Vec::new();
        file.take(max_bytes as u64).read_to_end(&mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).to_string())
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

    pub fn delete_path(&self, user_id: &str, path: &str) -> Result<()> {
        let target = self.resolve_path(user_id, path)?;
        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        } else if target.exists() {
            fs::remove_file(&target)?;
        }
        self.bump_version(user_id);
        Ok(())
    }

    pub fn create_dir(&self, user_id: &str, path: &str) -> Result<()> {
        let target = self.resolve_path(user_id, path)?;
        fs::create_dir_all(&target)?;
        self.bump_version(user_id);
        Ok(())
    }

    pub fn move_path(&self, user_id: &str, source: &str, destination: &str) -> Result<()> {
        let source_path = self.resolve_path(user_id, source)?;
        let destination_path = self.resolve_path(user_id, destination)?;
        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&source_path, &destination_path)?;
        self.bump_version(user_id);
        Ok(())
    }

    pub fn copy_path(&self, user_id: &str, source: &str, destination: &str) -> Result<()> {
        let source_path = self.resolve_path(user_id, source)?;
        let destination_path = self.resolve_path(user_id, destination)?;
        if source_path.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&source_path, &destination_path)?;
        }
        self.bump_version(user_id);
        Ok(())
    }

    pub fn search(&self, user_id: &str, keyword: &str) -> Result<Vec<WorkspaceEntry>> {
        let (entries, _total) =
            self.search_workspace_entries(user_id, keyword, 0, 0, true, true)?;
        Ok(entries)
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
        let safe_offset = offset.max(0);
        let safe_limit = limit.max(0);
        let mut matched = 0u64;
        let mut results = Vec::new();
        for entry in WalkDir::new(&root)
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
            if !name.to_lowercase().contains(&keyword) {
                continue;
            }
            matched += 1;
            if matched <= safe_offset {
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
                .strip_prefix(&root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            if safe_limit == 0 || results.len() < safe_limit as usize {
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
        Ok((results, matched))
    }

    pub fn archive(&self, user_id: &str, path: Option<&str>) -> Result<Vec<u8>> {
        let root = self.ensure_user_root(user_id)?;
        let target = match path {
            Some(path) if !path.is_empty() => self.resolve_path(user_id, path)?,
            _ => root.clone(),
        };
        let cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        let options = FileOptions::default();

        if target.is_dir() {
            for entry in WalkDir::new(&target)
                .into_iter()
                .filter_map(|entry| entry.ok())
            {
                let file_path = entry.path();
                if file_path.is_dir() {
                    continue;
                }
                let rel = file_path
                    .strip_prefix(&root)
                    .unwrap_or(file_path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let mut file = fs::File::open(file_path)?;
                zip.start_file(rel, options)?;
                let mut data = Vec::new();
                file.read_to_end(&mut data)?;
                zip.write_all(&data)?;
            }
        } else if target.is_file() {
            let rel = target
                .strip_prefix(&root)
                .unwrap_or(&target)
                .to_string_lossy()
                .replace('\\', "/");
            let mut file = fs::File::open(&target)?;
            zip.start_file(rel, options)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            zip.write_all(&data)?;
        }

        let cursor = zip.finish()?;
        Ok(cursor.into_inner())
    }

    pub fn get_workspace_tree(&self, user_id: &str) -> String {
        let safe_id = self.safe_user_id(user_id);
        {
            let cache = self.tree_cache.lock().unwrap();
            if let Some(tree) = cache.cache.get(&safe_id) {
                if !cache.dirty.contains(&safe_id) {
                    return tree.clone();
                }
            }
        }
        self.refresh_workspace_tree(user_id)
    }

    pub fn refresh_workspace_tree(&self, user_id: &str) -> String {
        let root = match self.ensure_user_root(user_id) {
            Ok(path) => path,
            Err(_) => return i18n::t("workspace.tree.empty"),
        };
        let tree = build_workspace_tree(&root, 2);
        let safe_id = self.safe_user_id(user_id);
        let mut cache = self.tree_cache.lock().unwrap();
        let previous = cache.cache.get(&safe_id).cloned();
        let dirty = cache.dirty.remove(&safe_id);
        if previous.as_deref() != Some(&tree) {
            cache.cache.insert(safe_id.clone(), tree.clone());
            if !dirty {
                self.increment_version(&safe_id);
            }
        }
        tree
    }

    pub fn mark_tree_dirty(&self, user_id: &str) {
        let safe_id = self.safe_user_id(user_id);
        self.increment_version(&safe_id);
        let mut cache = self.tree_cache.lock().unwrap();
        cache.dirty.insert(safe_id.clone());
        cache.cache.remove(&safe_id);
    }

    pub fn get_tree_version(&self, user_id: &str) -> u64 {
        let safe_id = self.safe_user_id(user_id);
        self.versions.get(&safe_id).map(|value| *value).unwrap_or(0)
    }

    pub fn tree_version(&self, user_id: &str) -> u64 {
        self.get_tree_version(user_id)
    }

    pub fn bump_version(&self, user_id: &str) {
        self.mark_tree_dirty(user_id);
    }

    fn increment_version(&self, safe_id: &str) {
        let mut entry = self.versions.entry(safe_id.to_string()).or_insert(0);
        *entry += 1;
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

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let new_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &new_path)?;
        } else {
            fs::copy(entry.path(), new_path)?;
        }
    }
    Ok(())
}

fn resolve_collision(path: &Path) -> PathBuf {
    let mut suffix = 1;
    let mut candidate = path.to_path_buf();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    while candidate.exists() {
        candidate = path.with_file_name(format!("{file_name}.migrated_{suffix}"));
        suffix += 1;
    }
    candidate
}

fn read_jsonl(path: &Path) -> Vec<Value> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    let mut records = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(payload) = serde_json::from_str::<Value>(trimmed) {
            records.push(payload);
        }
    }
    records
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
