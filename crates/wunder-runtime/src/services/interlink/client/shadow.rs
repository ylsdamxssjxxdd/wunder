//! Local shadow collector (docs §6.1, §6.2).
//!
//! Builds the metadata projection a node uploads as `shadow_full` /
//! `shadow_delta`: node summary, thread directory, scheduled tasks and a
//! bounded workspace tree. Red line: **no message bodies, no file contents, no
//! local absolute paths, no memory/knowledge payloads** - only identifiers,
//! titles, counts, sizes, timestamps and workspace-relative paths.
//!
//! Everything expensive (directory walk, thread directory, item counts) runs on
//! the blocking pool, never on the socket task or the UI thread, and every loop
//! is bounded *while* it walks (entry cap plus depth cap), not after.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde_json::{json, Value};

use wunder_core::interlink::{CAP_SHADOW_FULL, CAP_SHADOW_MINIMAL};

use crate::core::blocking;
use crate::services::thread_catalog::{ThreadCatalogService, ThreadListQuery};
use crate::state::AppState;

/// Directory tree depth cap (docs §6.1).
pub const TREE_DEPTH: usize = 3;
/// Directory tree entry cap (docs §6.1).
pub const TREE_MAX_ENTRIES: usize = 500;
/// Thread directory cap (docs §6.1).
pub const THREADS_MAX: usize = 200;
/// Scheduled task cap (docs §6.1).
pub const TASKS_MAX: usize = 100;
/// One catalog page; the catalog itself clamps a page to 100 rows.
const CATALOG_PAGE_SIZE: i64 = 100;
/// At most two pages are fetched per projection, so the cap is honoured
/// without an unbounded scan.
const CATALOG_MAX_PAGES: usize = 2;
/// Merge window for event-driven deltas (docs §6.2 2).
pub const DELTA_MERGE_WINDOW: Duration = Duration::from_secs(2);
/// Titles are truncated: a multi-kilobyte thread name must not inflate the
/// projection.
pub const TITLE_MAX_CHARS: usize = 96;
/// The interlink backup folder lives inside the workspace but is never
/// projected.
pub const BACKUP_DIR_NAME: &str = ".interlink_backup";

/// Section bit set of the projection; a delta carries only what changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sections(pub u8);

impl Sections {
    pub const SUMMARY: Sections = Sections(1);
    pub const THREADS: Sections = Sections(2);
    pub const TASKS: Sections = Sections(4);
    pub const WORKSPACE: Sections = Sections(8);
    pub const ALL: Sections = Sections(15);

    pub const fn empty() -> Sections {
        Sections(0)
    }

    pub fn union(self, other: Sections) -> Sections {
        Sections(self.0 | other.0)
    }

    pub fn contains(self, other: Sections) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Which shadow sections one executed command kind can have moved: the delta
/// window re-projects only what the command plausibly touched.
pub fn command_sections(kind: &str) -> Sections {
    if kind.starts_with("workspace.") {
        Sections::WORKSPACE
    } else if kind.starts_with("thread") {
        Sections::THREADS
    } else if kind.starts_with("node.") || kind.starts_with("shadow.") {
        Sections::SUMMARY
    } else {
        Sections::ALL
    }
}

/// Per-node static identity used in the summary section.
#[derive(Debug, Clone)]
pub struct NodeIdentity {
    pub client: String,
    pub device_name: String,
    pub app_version: String,
    pub engine_version: String,
}

impl NodeIdentity {
    /// Versions come from the engine crate: local forms report the same string
    /// the cloud channel registered the device with.
    pub fn from_session(client: &str, device_name: &str) -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
        Self {
            client: client.to_string(),
            device_name: device_name.to_string(),
            app_version: version.clone(),
            engine_version: version,
        }
    }
}

/// The §6.1 tree/thread/task budget, from `config.interlink.shadow`.
#[derive(Debug, Clone, Copy)]
pub struct TreeLimits {
    pub max_entries: usize,
    pub depth: usize,
    pub threads_max: usize,
    pub tasks_max: usize,
}

impl Default for TreeLimits {
    fn default() -> Self {
        Self {
            max_entries: TREE_MAX_ENTRIES,
            depth: TREE_DEPTH,
            threads_max: THREADS_MAX,
            tasks_max: TASKS_MAX,
        }
    }
}

/// What the collector needs to know about the node, resolved once per
/// connection so no section re-reads the session file.
#[derive(Debug, Clone)]
pub struct ShadowSources {
    pub identity: NodeIdentity,
    /// Local engine user whose threads and workspace are projected.
    pub local_user_id: String,
    /// Desktop/CLI workspace binding; `None` uses the default scope.
    pub workspace_id: Option<String>,
    pub os: String,
    pub arch: String,
    pub limits: TreeLimits,
    /// Privacy downgrade (docs §6.2 5): only the summary is projected.
    pub minimal: bool,
}

impl ShadowSources {
    /// Resolve the source set from the capabilities the server actually granted
    /// (`hello_ack.capabilities_granted`): `shadow:full` unlocks the thread
    /// directory and the tree, anything less is the minimal projection.
    pub fn resolved(
        identity: NodeIdentity,
        local_user_id: String,
        workspace_id: Option<String>,
        limits: TreeLimits,
        capabilities: &[String],
    ) -> Self {
        // The grant level is decided by the strongest shadow capability the
        // server authorized: `shadow:full` upgrades the projection even when
        // the default `shadow:minimal` grant is still in the list. This must
        // stay in sync with the hello_ack decision in the connect loop.
        let minimal = !capabilities.iter().any(|cap| cap == CAP_SHADOW_FULL);
        Self {
            identity,
            local_user_id,
            workspace_id,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            limits,
            minimal,
        }
    }
}

/// One tree row: workspace-relative path only.
#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub path: String,
    pub kind: &'static str,
    pub size: u64,
    pub mtime: f64,
}

impl TreeEntry {
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "kind": self.kind,
            "size": self.size,
            "mtime": self.mtime,
        })
    }
}

/// Result of a bounded tree walk.
#[derive(Debug, Default)]
pub struct TreeWalk {
    pub entries: Vec<TreeEntry>,
    pub truncated: bool,
    /// Directories that could not be read (permissions, unmounted roots).
    pub unreadable: usize,
}

/// Walk the workspace tree breadth-first with a hard entry cap and depth cap.
///
/// The caps are applied *while* walking: a huge folder never gets enumerated
/// and then trimmed, the walk stops at `max_entries`. Hidden entries and the
/// interlink backup folder are skipped, symlinks are not followed, and every
/// emitted path is relative to `root`.
pub fn walk_tree(root: &Path, limits: &TreeLimits) -> TreeWalk {
    let max_entries = limits.max_entries.clamp(1, 5_000);
    let max_depth = limits.depth.clamp(1, 6);
    let mut walk = TreeWalk::default();
    if !root.is_dir() {
        return walk;
    }
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), 1));
    while let Some((dir, depth)) = queue.pop_front() {
        let read = match std::fs::read_dir(&dir) {
            Ok(read) => read,
            Err(_) => {
                walk.unreadable += 1;
                continue;
            }
        };
        for item in read.flatten() {
            if walk.entries.len() >= max_entries {
                walk.truncated = true;
                return walk;
            }
            let name = item.file_name().to_string_lossy().to_string();
            if is_excluded(&name) {
                continue;
            }
            let path = item.path();
            let metadata = match item.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let is_dir = metadata.is_dir();
            walk.entries.push(TreeEntry {
                path: relative_of(root, &path),
                kind: if is_dir { "dir" } else { "file" },
                size: if is_dir { 0 } else { metadata.len() },
                mtime: unix_time_of(&metadata),
            });
            // Only descend while the child level is still within the budget.
            if is_dir && depth < max_depth {
                queue.push_back((path, depth + 1));
            }
        }
    }
    walk
}

fn is_excluded(name: &str) -> bool {
    name.starts_with('.') || name == BACKUP_DIR_NAME
}

/// Workspace-relative, slash-separated path of one entry; an entry that is not
/// under the root yields an empty path and is dropped by the caller.
fn relative_of(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn unix_time_of(metadata: &std::fs::Metadata) -> f64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs_f64())
        .unwrap_or(0.0)
}

/// One thread directory row (no message bodies, ever).
#[derive(Debug, Clone)]
pub struct ThreadRowSource {
    pub local_thread_id: String,
    pub title: String,
    pub status: String,
    pub agent: Option<String>,
    pub updated_at: f64,
    pub message_count: i64,
    pub duration_ms: i64,
}

/// Shape the thread directory and report whether it was truncated.
pub fn thread_rows(rows: Vec<ThreadRowSource>, max: usize) -> (Vec<Value>, bool) {
    let truncated = rows.len() > max;
    let mut items = Vec::with_capacity(rows.len().min(max));
    for row in rows.into_iter().take(max) {
        items.push(json!({
            "local_thread_id": row.local_thread_id,
            "title": row.title.chars().take(TITLE_MAX_CHARS).collect::<String>(),
            "status": row.status,
            "agent": row.agent,
            "updated_at": row.updated_at,
            "message_count": row.message_count.max(0),
            "duration_ms": row.duration_ms.max(0),
        }));
    }
    (items, truncated)
}

/// Shape the scheduled-task directory; the id set is de-duplicated so a
/// replayed delta cannot inflate the row count.
pub fn task_rows(tasks: Vec<Value>, max: usize) -> (Vec<Value>, bool) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut items = Vec::with_capacity(max.min(tasks.len()));
    for task in tasks {
        let id = task
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        if items.len() >= max {
            return (items, true);
        }
        items.push(json!({
            "id": id,
            "name": task
                .get("name")
                .and_then(Value::as_str)
                .map(|value| value.chars().take(TITLE_MAX_CHARS).collect::<String>()),
            "schedule": task.get("schedule").cloned().unwrap_or(Value::Null),
            "enabled": task.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            "next_run_at": task.get("next_run_at").cloned().unwrap_or(Value::Null),
        }));
    }
    (items, false)
}

/// Node summary section (docs §6.1 rows 1 and 5).
pub fn summary_json(
    identity: &NodeIdentity,
    os: &str,
    arch: &str,
    active_threads: usize,
    queued: usize,
    last_error: Option<&str>,
) -> Value {
    json!({
        "os": os,
        "arch": arch,
        "client": identity.client,
        "device_name": identity.device_name,
        "app_version": identity.app_version,
        "engine_version": identity.engine_version,
        "active_threads": active_threads,
        "queued": queued,
        "last_error": last_error.map(|text| text.chars().take(160).collect::<String>()),
    })
}

/// Tracks the revision counter, the dirty sections and the cheap in-memory
/// change signals. No filesystem watch: a local change reaches the projection
/// through the engine's own counters (docs §6.2 2).
#[derive(Debug)]
pub struct ShadowCollector {
    revision: AtomicU64,
    dirty: RwLock<Sections>,
    signals: RwLock<Signals>,
    minimal: RwLock<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Signals {
    tree_version: u64,
    active_threads: u64,
}

impl Default for ShadowCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ShadowCollector {
    pub fn new() -> Self {
        Self {
            revision: AtomicU64::new(1),
            dirty: RwLock::new(Sections::empty()),
            signals: RwLock::new(Signals::default()),
            minimal: RwLock::new(false),
        }
    }

    /// Start a fresh connection: the first `shadow_full` is revision 1 and the
    /// merge window forgets whatever it had queued (docs §6.2 1).
    pub fn reset(&self, minimal: bool) -> i64 {
        *self.minimal.write().expect("shadow collector lock poisoned") = minimal;
        *self.dirty.write().expect("shadow collector lock poisoned") = Sections::empty();
        *self.signals.write().expect("shadow collector lock poisoned") = Signals::default();
        self.revision.store(1, Ordering::Release);
        1
    }

    pub fn revision(&self) -> i64 {
        self.revision.load(Ordering::Acquire) as i64
    }

    /// Claim the next revision for a delta or a periodic full upload.
    pub fn next_revision(&self) -> i64 {
        self.revision.fetch_add(1, Ordering::AcqRel) as i64 + 1
    }

    pub fn is_minimal(&self) -> bool {
        *self.minimal.read().expect("shadow collector lock poisoned")
    }

    /// Flag sections as changed (called by local mutations and remote commands).
    pub fn mark_dirty(&self, sections: Sections) {
        let mut dirty = self.dirty.write().expect("shadow collector lock poisoned");
        *dirty = dirty.union(sections);
    }

    pub fn take_dirty(&self) -> Sections {
        let mut dirty = self.dirty.write().expect("shadow collector lock poisoned");
        std::mem::replace(&mut *dirty, Sections::empty())
    }

    pub fn has_dirty(&self) -> bool {
        !self.dirty.read().expect("shadow collector lock poisoned").is_empty()
    }

    /// Compare the cheap signals against the last sample and flag what moved.
    pub fn refresh_signals(&self, tree_version: u64, active_threads: u64) -> Sections {
        let mut changed = Sections::empty();
        {
            let mut signals = self.signals.write().expect("shadow collector lock poisoned");
            if signals.tree_version != tree_version {
                changed = changed.union(Sections::WORKSPACE);
                signals.tree_version = tree_version;
            }
            if signals.active_threads != active_threads {
                changed = changed.union(Sections::THREADS.union(Sections::SUMMARY));
                signals.active_threads = active_threads;
            }
        }
        if !changed.is_empty() {
            self.mark_dirty(changed);
        }
        changed
    }
}

/// The workspace scope a node projects: the plain local user, or the desktop
/// workspace binding when the caller supplied one. Fence, path resolution and
/// size limits stay inside `WorkspaceManager` - this only picks the scope key.
pub fn workspace_scope(state: &Arc<AppState>, sources: &ShadowSources) -> String {
    let user = sources.local_user_id.trim();
    if user.is_empty() {
        return String::new();
    }
    match sources.workspace_id.as_deref().map(str::trim) {
        Some(workspace_id) if !workspace_id.is_empty() => state
            .workspace
            .scoped_user_id_for_workspace(user, workspace_id),
        _ => state.workspace.scoped_user_id(user, None),
    }
}

/// Build the `threads` section from the durable catalog: at most two bounded
/// pages, plus one batched item-count pass. No message content is read.
pub async fn gather_threads(
    state: &Arc<AppState>,
    sources: &ShadowSources,
    limits: &TreeLimits,
) -> (Vec<Value>, bool) {
    let want = limits.threads_max.max(1);
    let rows = collect_thread_rows(state, &sources.local_user_id, want).await;
    thread_rows(rows, want)
}

async fn collect_thread_rows(
    state: &Arc<AppState>,
    user_id: &str,
    want: usize,
) -> Vec<ThreadRowSource> {
    let mut rows: Vec<ThreadRowSource> = Vec::with_capacity(want.min(THREADS_MAX));
    let catalog = ThreadCatalogService::new(state.as_ref().clone());
    for page in 0..CATALOG_MAX_PAGES {
        if rows.len() >= want {
            break;
        }
        let query = ThreadListQuery {
            user_id: user_id.to_string(),
            offset: page as i64 * CATALOG_PAGE_SIZE,
            limit: CATALOG_PAGE_SIZE.min(want as i64),
            search: None,
            parent_session_id: None,
            workspace_id: None,
        };
        let Ok(page_result) = catalog.list(query).await else {
            break;
        };
        let took = page_result.items.len();
        for item in page_result.items {
            if rows.len() >= want {
                break;
            }
            rows.push(ThreadRowSource {
                local_thread_id: item.session_id.clone(),
                title: item.title.clone(),
                status: item.status.as_str().to_string(),
                agent: item.agent_id.clone(),
                updated_at: item.updated_at,
                message_count: 0,
                duration_ms: 0,
            });
        }
        if took < CATALOG_PAGE_SIZE as usize {
            break;
        }
    }
    if rows.is_empty() {
        return rows;
    }
    let ids: Vec<String> = rows.iter().map(|row| row.local_thread_id.clone()).collect();
    let storage = state.storage.clone();
    let owner = user_id.to_string();
    let counts = blocking::run_db("interlink.client.shadow_counts", move || {
        // One bounded batch: an indexed COUNT per thread, never a scan of the
        // transcript.
        let mut out = Vec::with_capacity(ids.len());
        for id in &ids {
            let items = storage
                .get_thread_log_counts(&owner, id, false)
                .map(|(_, items)| items)
                .unwrap_or(0);
            out.push(items);
        }
        Ok::<Vec<i64>, anyhow::Error>(out)
    })
    .await
    .unwrap_or_default();
    for (row, items) in rows.iter_mut().zip(counts) {
        row.message_count = items;
    }
    // Duration is only known for threads this node is running right now; the
    // projection carries 0 for the rest instead of reading message bodies.
    let live = live_durations(state);
    for row in rows.iter_mut() {
        if let Some(ms) = live.get(&row.local_thread_id) {
            row.duration_ms = *ms;
        }
    }
    rows
}

/// Active sessions of this node: only the id and the elapsed time are read out
/// of the monitor summary, the rest of the record (including the question
/// text) is never touched.
fn live_durations(state: &Arc<AppState>) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    for record in state.monitor.list_sessions(true) {
        let Some(id) = record.get("session_id").and_then(Value::as_str) else {
            continue;
        };
        let elapsed = record.get("elapsed_s").and_then(Value::as_f64).unwrap_or(0.0);
        out.insert(id.to_string(), (elapsed * 1_000.0).max(0.0) as i64);
    }
    out
}

/// Build the `tasks` section from the durable cron ledger.
pub async fn gather_tasks(
    state: &Arc<AppState>,
    sources: &ShadowSources,
    limits: &TreeLimits,
) -> (Vec<Value>, bool) {
    let want = limits.tasks_max.max(1);
    let storage = state.storage.clone();
    let owner = sources.local_user_id.clone();
    let records = blocking::run_db("interlink.client.shadow_tasks", move || {
        let jobs = storage
            .list_cron_jobs(&owner, true)
            .unwrap_or_default()
            .into_iter()
            .take(want + 1)
            .map(|job| {
                json!({
                    "id": job.job_id,
                    "name": job.name,
                    "schedule": {
                        "kind": job.schedule_kind,
                        "cron": job.schedule_cron,
                        "every_ms": job.schedule_every_ms,
                        "at": job.schedule_at,
                        "tz": job.schedule_tz,
                    },
                    "enabled": job.enabled,
                    "next_run_at": job.next_run_at,
                })
            })
            .collect::<Vec<_>>();
        Ok::<Vec<Value>, anyhow::Error>(jobs)
    })
    .await
    .unwrap_or_default();
    task_rows(records, want)
}

/// Build the workspace section: bounded tree walk plus the cached usage
/// summary of the same scope.
pub async fn gather_workspace(
    state: &Arc<AppState>,
    sources: &ShadowSources,
    limits: &TreeLimits,
) -> (Value, bool) {
    let scope = workspace_scope(state, sources);
    let root = state.workspace.workspace_root(&scope);
    let walk_limits = TreeLimits {
        max_entries: limits.max_entries,
        depth: limits.depth,
        threads_max: 1,
        tasks_max: 1,
    };
    let walk = blocking::run_fs("interlink.client.shadow_tree", move || {
        Ok::<TreeWalk, anyhow::Error>(walk_tree(&root, &walk_limits))
    })
    .await
    .unwrap_or_default();
    let usage = usage_summary(state, &scope).await;
    let mut workspace = json!({
        "tree": walk.entries.iter().map(TreeEntry::to_json).collect::<Vec<_>>(),
        "usage": usage,
        "roots": 1,
    });
    if walk.truncated {
        workspace["truncated"] = json!(true);
    }
    (workspace, walk.truncated)
}

async fn usage_summary(state: &Arc<AppState>, scope: &str) -> Value {
    let workspace = state.workspace.clone();
    let owner = scope.to_string();
    match workspace.workspace_usage_summary_async(&owner, ".", 1).await {
        Ok(summary) => json!({
            "files": summary.files,
            "dirs": summary.dirs,
            "used_bytes": summary.used_bytes,
            "truncated": summary.truncated,
        }),
        Err(_) => json!({}),
    }
}

/// Live counters folded into the summary section.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeCounters {
    pub active_threads: usize,
    pub queued: usize,
}

/// Assemble a full `shadow_full` payload (docs §6.2 1). Under the minimal
/// grant only the summary is uploaded (docs §6.2 5).
pub async fn build_full(
    state: &Arc<AppState>,
    sources: &ShadowSources,
    collector: &ShadowCollector,
    revision: i64,
    counters: RuntimeCounters,
    last_error: Option<&str>,
) -> Value {
    let summary = summary_json(
        &sources.identity,
        &sources.os,
        &sources.arch,
        counters.active_threads,
        counters.queued,
        last_error,
    );
    if sources.minimal || collector.is_minimal() {
        return json!({ "revision": revision, "summary": summary });
    }
    let limits = sources.limits;
    let (threads, threads_truncated) = gather_threads(state, sources, &limits).await;
    let (tasks, tasks_truncated) = gather_tasks(state, sources, &limits).await;
    let (mut workspace, tree_truncated) = gather_workspace(state, sources, &limits).await;
    if threads_truncated || tasks_truncated || tree_truncated {
        workspace["truncated"] = json!(true);
    }
    json!({
        "revision": revision,
        "summary": summary,
        "threads": threads,
        "tasks": tasks,
        "workspace": workspace,
    })
}

/// Assemble a `shadow_delta` payload carrying exactly the dirty sections.
/// Returns `None` when nothing is pending.
pub async fn build_delta(
    state: &Arc<AppState>,
    sources: &ShadowSources,
    collector: &ShadowCollector,
    sections: Sections,
    revision: i64,
    counters: RuntimeCounters,
    last_error: Option<&str>,
) -> Option<Value> {
    if sections.is_empty() {
        return None;
    }
    let mut payload = json!({ "revision": revision });
    if sections.contains(Sections::SUMMARY) {
        payload["summary"] = summary_json(
            &sources.identity,
            &sources.os,
            &sources.arch,
            counters.active_threads,
            counters.queued,
            last_error,
        );
    }
    if sources.minimal || collector.is_minimal() {
        return Some(payload);
    }
    let limits = sources.limits;
    if sections.contains(Sections::THREADS) {
        let (threads, truncated) = gather_threads(state, sources, &limits).await;
        payload["threads"] = json!(threads);
        if truncated {
            payload["truncated"] = json!(true);
        }
    }
    if sections.contains(Sections::TASKS) {
        let (tasks, truncated) = gather_tasks(state, sources, &limits).await;
        payload["tasks"] = json!(tasks);
        if truncated {
            payload["truncated"] = json!(true);
        }
    }
    if sections.contains(Sections::WORKSPACE) {
        let (workspace, truncated) = gather_workspace(state, sources, &limits).await;
        payload["workspace"] = workspace;
        if truncated {
            payload["truncated"] = json!(true);
        }
    }
    Some(payload)
}
