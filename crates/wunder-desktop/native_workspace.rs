//! Workspace façade: bind threads to real user folders.
//!
//! Workspaces are the desktop's only organizational unit. Every thread lives
//! in exactly one workspace and its tool execution root is the bound folder,
//! so all file work happens directly in the user's project directory. This
//! layer owns CRUD, path validation and the one-time local migration; storage
//! and execution semantics stay in the shared engine.

use super::{NativeDesktop, NativeInterlinkEntry, NativeInterlinkListing};
use anyhow::{anyhow, bail, Result};
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, HashMap},
    io::Read,
    path::{Component, Path, PathBuf},
};
use wunder_server::interlink::client::shadow::Sections;
use wunder_server::storage::{WorkspaceRecord, USER_PRIVATE_CONTAINER_ID};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;

pub const WORKSPACE_ROOT_MISSING: &str = "工作区文件夹不存在或不可访问";

struct LocalListingCandidate(NativeInterlinkEntry);
impl PartialEq for LocalListingCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for LocalListingCandidate {}
impl PartialOrd for LocalListingCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for LocalListingCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        (other.0.kind == "dir")
            .cmp(&(self.0.kind == "dir"))
            .then_with(|| natural_name_cmp(&self.0.name, &other.0.name))
            .then_with(|| self.0.name.cmp(&other.0.name))
    }
}

/// Compare numeric runs without parsing into machine integers. This keeps
/// sample2 before sample10 and supports arbitrarily long numeric file names.
fn natural_name_cmp(left: &str, right: &str) -> Ordering {
    let mut left = left.chars().flat_map(char::to_lowercase).peekable();
    let mut right = right.chars().flat_map(char::to_lowercase).peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let mut a = String::new();
                let mut b = String::new();
                while left.peek().is_some_and(char::is_ascii_digit) {
                    a.push(left.next().unwrap());
                }
                while right.peek().is_some_and(char::is_ascii_digit) {
                    b.push(right.next().unwrap());
                }
                let a = a.trim_start_matches('0');
                let b = b.trim_start_matches('0');
                let order = a.len().cmp(&b.len()).then_with(|| a.cmp(b));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(a), Some(b)) => {
                left.next();
                right.next();
                if a != b {
                    return a.cmp(&b);
                }
            }
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
        }
    }
}

/// Icon choices offered by the workspace dialog; stored values outside this
/// set fall back to the default icon at read time.
pub const WORKSPACE_ICONS: &[&str] = &[
    "folder", "monitor", "martini", "cake", "cookie", "pyramid", "alien", "camera", "gear",
    "coffee", "heart", "robot", "ghost", "bell", "flask",
];

/// Palette choices offered by the workspace dialog.
pub const WORKSPACE_COLORS: &[&str] = &[
    "red",
    "green",
    "blue",
    "brown",
    "crimson",
    "pink",
    "purple",
    "yellow",
    "orange",
    "teal",
    "bright-blue",
    "gray",
    "black",
    "light-purple",
    "light-green",
];

#[derive(Clone, Debug)]
pub struct NativeWorkspace {
    pub workspace_id: String,
    pub name: String,
    /// Absolute host folder; tool execution and file access are rooted here.
    pub root_path: String,
    pub icon: String,
    pub color: String,
    pub sort_index: i64,
    pub created_at: f64,
    pub updated_at: f64,
    /// Non-archived threads bound to this workspace.
    pub thread_count: i64,
}

#[derive(Clone, Debug)]
pub struct WorkspacePathReport {
    pub path: String,
    pub exists: bool,
    pub is_directory: bool,
    pub readable: bool,
    pub writable: bool,
    /// Workspace already bound to this exact folder.
    pub occupied_by: Option<String>,
    /// Workspace whose folder contains this path.
    pub nested_in: Option<String>,
    /// Workspace folder inside this path.
    pub contains: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceDeleteSummary {
    pub archived_threads: u64,
    pub deleted_threads: u64,
}

/// Input for create/update; `workspace_id` empty means create.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceEdit {
    pub workspace_id: String,
    pub name: String,
    pub root_path: String,
    pub icon: String,
    pub color: String,
}

impl NativeDesktop {
    pub fn list_workspaces(&self) -> Result<Vec<NativeWorkspace>> {
        let storage = self.state().storage.as_ref();
        let records = storage.list_workspaces(self.user_id())?;
        let counts: HashMap<String, i64> = storage
            .count_chat_sessions_by_workspace(self.user_id())?
            .into_iter()
            .collect();
        Ok(records
            .into_iter()
            .map(|record| {
                let thread_count = counts
                    .get(&record.workspace_id)
                    .copied()
                    .unwrap_or_default();
                native_workspace(&record, thread_count)
            })
            .collect())
    }

    pub fn get_workspace(&self, workspace_id: &str) -> Result<NativeWorkspace> {
        let cleaned = workspace_id.trim();
        if cleaned.is_empty() {
            bail!("工作区不存在");
        }
        let record = self
            .state()
            .storage
            .get_workspace(self.user_id(), cleaned)?
            .ok_or_else(|| anyhow!("工作区不存在"))?;
        let thread_count = self
            .state()
            .storage
            .count_chat_sessions_by_workspace(self.user_id())?
            .into_iter()
            .find(|(id, _)| id == cleaned)
            .map(|(_, count)| count)
            .unwrap_or_default();
        Ok(native_workspace(&record, thread_count))
    }

    pub fn create_workspace(
        &self,
        name: &str,
        root_path: &str,
        icon: &str,
        color: &str,
    ) -> Result<NativeWorkspace> {
        let record = self.store_workspace(&WorkspaceEdit {
            name: name.to_string(),
            root_path: root_path.to_string(),
            icon: icon.to_string(),
            color: color.to_string(),
            ..WorkspaceEdit::default()
        })?;
        self.publish_workspace_change(&record);
        Ok(native_workspace(&record, 0))
    }

    pub fn update_workspace(&self, edit: &WorkspaceEdit) -> Result<NativeWorkspace> {
        let record = self.store_workspace(edit)?;
        self.publish_workspace_change(&record);
        let thread_count = self
            .state()
            .storage
            .count_chat_sessions_by_workspace(self.user_id())?
            .into_iter()
            .find(|(id, _)| id == &record.workspace_id)
            .map(|(_, count)| count)
            .unwrap_or_default();
        Ok(native_workspace(&record, thread_count))
    }

    /// Delete a workspace folder binding only. Threads are archived or their
    /// records removed per the caller's choice; disk contents are never
    /// touched.
    pub fn delete_workspace(
        &self,
        workspace_id: &str,
        delete_threads: bool,
    ) -> Result<WorkspaceDeleteSummary> {
        let cleaned = workspace_id.trim();
        if cleaned.is_empty() {
            bail!("工作区不存在");
        }
        let storage = self.state().storage.as_ref();
        storage
            .get_workspace(self.user_id(), cleaned)?
            .ok_or_else(|| anyhow!("工作区不存在"))?;
        let mut summary = WorkspaceDeleteSummary::default();
        if delete_threads {
            summary.deleted_threads = self.discard_workspace_threads(cleaned)? as u64;
        } else {
            summary.archived_threads = self.archive_workspace_threads(cleaned)? as u64;
        }
        storage.delete_workspace(self.user_id(), cleaned)?;
        self.state().workspace.unregister_workspace_root(cleaned);
        self.invalidate_workspace_projection();
        Ok(summary)
    }

    /// Keep the interlink tunnel in step with workspace CRUD: the real folder
    /// becomes reachable for remote commands right away, and the shadow's
    /// workspace section re-projects on the next merge window so the cloud
    /// workspace list stays current without waiting for the periodic full.
    fn publish_workspace_change(&self, record: &WorkspaceRecord) {
        self.state()
            .workspace
            .register_workspace_root(&record.workspace_id, &record.root_path);
        self.invalidate_workspace_projection();
    }

    fn invalidate_workspace_projection(&self) {
        if let Some(client) = wunder_server::interlink::client::shared() {
            client.invalidate_shadow(Sections::WORKSPACE);
        }
    }

    /// Persist sidebar ordering; ids not present keep their previous index.
    pub fn reorder_workspaces(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let storage = self.state().storage.as_ref();
        let mut known: HashMap<String, WorkspaceRecord> = storage
            .list_workspaces(self.user_id())?
            .into_iter()
            .map(|record| (record.workspace_id.clone(), record))
            .collect();
        for (index, id) in ids.iter().enumerate() {
            let Some(mut record) = known.remove(id.trim()) else {
                continue;
            };
            record.sort_index = index as i64;
            record.updated_at = now_ts();
            storage.upsert_workspace(&record)?;
        }
        Ok(())
    }

    /// Validate a candidate folder before create/update. Read/write probes
    /// use temporary names inside the directory and clean up after themselves.
    pub fn validate_workspace_path(&self, raw_path: &str) -> Result<WorkspacePathReport> {
        self.validate_workspace_edit_path(raw_path, "")
    }

    /// Validate an edited binding without treating its own folder as occupied.
    /// Other workspace bindings keep the same overlap and permission checks.
    pub fn validate_workspace_edit_path(
        &self,
        raw_path: &str,
        workspace_id: &str,
    ) -> Result<WorkspacePathReport> {
        let path = raw_path.trim();
        let mut report = WorkspacePathReport {
            path: path.to_string(),
            exists: false,
            is_directory: false,
            readable: false,
            writable: false,
            occupied_by: None,
            nested_in: None,
            contains: None,
        };
        if path.is_empty() {
            return Ok(report);
        }
        let Ok(root) = normalize_root(path) else {
            return Ok(report);
        };
        report.exists = true;
        report.is_directory = root.is_dir();
        if !report.is_directory {
            return Ok(report);
        }
        report.readable = std::fs::read_dir(&root).is_ok();
        report.writable = probe_writable(&root);
        for record in self.state().storage.list_workspaces(self.user_id())? {
            if record.workspace_id == workspace_id {
                continue;
            }
            let other = workspace_canonical(Path::new(&record.root_path));
            if paths_equal(&other, &root) {
                report.occupied_by = Some(record.name);
            } else if workspace_contains(&root, &other) {
                report.contains = Some(record.name);
            } else if workspace_contains(&other, &root) {
                report.nested_in = Some(record.name);
            }
        }
        Ok(report)
    }

    /// Resolve and canonicalize a workspace folder for create/update.
    fn resolve_new_root(&self, raw_path: &str, exclude_id: &str) -> Result<PathBuf> {
        let path = raw_path.trim();
        if path.is_empty() {
            bail!("请选择工作区文件夹");
        }
        let root = normalize_root(path).map_err(|_| anyhow!(WORKSPACE_ROOT_MISSING))?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        if !probe_writable(&root) {
            bail!("文件夹没有写入权限");
        }
        for record in self.state().storage.list_workspaces(self.user_id())? {
            if record.workspace_id == exclude_id {
                continue;
            }
            let other = workspace_canonical(Path::new(&record.root_path));
            if paths_equal(&other, &root) {
                bail!("该文件夹已被工作区「{}」使用", record.name);
            }
            if workspace_contains(&root, &other) || workspace_contains(&other, &root) {
                bail!("工作区文件夹不能嵌套其他工作区文件夹");
            }
        }
        Ok(root)
    }

    fn store_workspace(&self, edit: &WorkspaceEdit) -> Result<WorkspaceRecord> {
        let name = edit.name.trim();
        if name.is_empty() || name.chars().count() > 60 {
            bail!("工作区名称无效");
        }
        if name.chars().any(char::is_control) {
            bail!("工作区名称无效");
        }
        let storage = self.state().storage.as_ref();
        let creating = edit.workspace_id.trim().is_empty();
        let now = now_ts();
        let record = if creating {
            let root = self.resolve_new_root(&edit.root_path, "")?;
            if storage
                .find_workspace_by_root(self.user_id(), &root.to_string_lossy())?
                .is_some()
            {
                bail!("该文件夹已被其他工作区使用");
            }
            WorkspaceRecord {
                workspace_id: format!("ws_{}", uuid::Uuid::new_v4().simple()),
                user_id: self.user_id().to_string(),
                name: name.to_string(),
                root_path: root.to_string_lossy().into_owned(),
                icon: normalize_choice(&edit.icon, "folder", WORKSPACE_ICONS),
                color: normalize_choice(&edit.color, "blue", WORKSPACE_COLORS),
                sort_index: storage.next_workspace_sort_index(self.user_id())?,
                created_at: now,
                updated_at: now,
            }
        } else {
            let workspace_id = edit.workspace_id.trim();
            let mut record = storage
                .get_workspace(self.user_id(), workspace_id)?
                .ok_or_else(|| anyhow!("工作区不存在"))?;
            let root = self.resolve_new_root(&edit.root_path, workspace_id)?;
            record.name = name.to_string();
            record.root_path = root.to_string_lossy().into_owned();
            record.icon = normalize_choice(&edit.icon, &record.icon, WORKSPACE_ICONS);
            record.color = normalize_choice(&edit.color, &record.color, WORKSPACE_COLORS);
            record.updated_at = now;
            record
        };
        storage.upsert_workspace(&record)?;
        Ok(record)
    }

    fn archive_workspace_threads(&self, workspace_id: &str) -> Result<usize> {
        let storage = self.state().storage.as_ref();
        let mut archived = 0;
        const PAGE: i64 = 200;
        loop {
            // Each write removes a row from the active query. Always drain
            // its first page; advancing an offset would skip remaining rows.
            let (records, _) = storage.list_chat_sessions_by_workspace(
                self.user_id(),
                workspace_id,
                Some("active"),
                0,
                PAGE,
            )?;
            let page = records.len();
            for mut record in records {
                record.status = "archived".into();
                record.updated_at = now_ts();
                storage.upsert_chat_session(&record)?;
                archived += 1;
            }
            if page < PAGE as usize {
                break;
            }
        }
        Ok(archived)
    }

    fn discard_workspace_threads(&self, workspace_id: &str) -> Result<usize> {
        let storage = self.state().storage.as_ref();
        let mut deleted = 0;
        const PAGE: i64 = 200;
        loop {
            let (records, _) = storage.list_chat_sessions_by_workspace(
                self.user_id(),
                workspace_id,
                None,
                0,
                PAGE,
            )?;
            let page = records.len();
            for record in records {
                storage.delete_chat_session(self.user_id(), &record.session_id)?;
                deleted += 1;
            }
            if page < PAGE as usize {
                break;
            }
        }
        Ok(deleted)
    }

    /// Open one workspace file with the OS default application. Paths are
    /// relative to the workspace root and cannot escape it. Persisted chat
    /// attachments resolve to the controlled private workspace instead.
    pub fn open_workspace_resource(&self, root_path: &str, relative: &str) -> Result<()> {
        let target = if let Some(target) = self.chat_attachment_target(relative)? {
            target
        } else {
            // Same root form as every other workspace entry point: a raw
            // `PathBuf::from` here kept the caller's spelling and made the
            // containment check reject perfectly valid nested files.
            let root = normalize_root(root_path)?;
            if !root.is_dir() {
                bail!(WORKSPACE_ROOT_MISSING);
            }
            workspace_resource_target(&root, relative)?
        };
        if !target.exists() {
            bail!("文件不存在");
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            #[link(name = "shell32")]
            unsafe extern "system" {
                fn ShellExecuteW(
                    window: isize,
                    operation: *const u16,
                    file: *const u16,
                    parameters: *const u16,
                    directory: *const u16,
                    show: i32,
                ) -> isize;
            }
            let file: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
            let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
            let result = unsafe {
                ShellExecuteW(
                    0,
                    operation.as_ptr(),
                    file.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    1,
                )
            };
            if result <= 32 {
                bail!("系统程序无法打开文件（错误码 {result}）");
            }
        }
        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open").arg(&target).spawn()?;
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open").arg(&target).spawn()?;
        }
        Ok(())
    }

    /// Resolve one workspace-relative path to its absolute host file for the
    /// drag-out gesture. The path must exist inside the workspace root.
    pub fn workspace_local_resolve(&self, root_path: &str, relative: &str) -> Result<PathBuf> {
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let target = workspace_resource_target(&root, relative)?;
        if !target.is_file() {
            bail!("只有文件支持拖出");
        }
        Ok(target)
    }

    /// Create one directory inside the workspace. The name is a single path
    /// segment; the parent may be nested ("notes/archive").
    pub fn workspace_local_mkdir(&self, root_path: &str, relative: &str) -> Result<()> {
        let target = self.workspace_new_target(root_path, relative)?;
        std::fs::create_dir_all(&target).map_err(|error| anyhow!("创建目录失败：{error}"))?;
        Ok(())
    }

    /// Create one empty file inside the workspace. An existing entry is never
    /// overwritten — the cloud write semantics do not apply to disk content.
    pub fn workspace_local_write(&self, root_path: &str, relative: &str) -> Result<()> {
        let target = self.workspace_new_target(root_path, relative)?;
        if target.exists() {
            bail!("同名文件或目录已存在");
        }
        std::fs::write(&target, b"").map_err(|error| anyhow!("创建文件失败：{error}"))?;
        Ok(())
    }

    /// Copy one dropped OS file into the workspace root (drag-in upload for a
    /// local folder). The stored name is the file's own; an existing entry is
    /// replaced, matching the cloud write semantics.
    pub fn workspace_local_upload(&self, root_path: &str, source: &Path) -> Result<String> {
        self.workspace_local_upload_to(root_path, "", source)
    }

    /// Copy a dropped file into the selected workspace-relative directory.
    /// Resolving both the parent and existing destination prevents symlinks
    /// from redirecting a local copy outside the workspace, and copying a file
    /// onto itself is a no-op rather than a destructive truncation.
    pub fn workspace_local_upload_to(
        &self,
        root_path: &str,
        directory: &str,
        source: &Path,
    ) -> Result<String> {
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let meta = std::fs::metadata(source).map_err(|error| anyhow!("读取失败：{error}"))?;
        if meta.is_dir() {
            bail!("目录不支持直接上传");
        }
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| anyhow!("无效的文件名"))?;
        validate_entry_name(&name)?;
        let parent = if directory.is_empty() {
            root.clone()
        } else {
            workspace_resource_target(&root, directory)?
        };
        if !parent.is_dir() {
            bail!("目标目录不存在");
        }
        let target = parent.join(&name);
        if target.exists() {
            if !workspace_contains(&root, &target) {
                bail!("路径超出当前工作区");
            }
            // Copying a file onto itself must not truncate it; compare the
            // same identity the workspace uses elsewhere (casing, links).
            if paths_equal(&target, source) {
                return Ok(if directory.is_empty() {
                    name
                } else {
                    format!("{directory}/{name}")
                });
            }
        }
        std::fs::copy(source, &target).map_err(|error| anyhow!("复制失败：{error}"))?;
        Ok(if directory.is_empty() {
            name
        } else {
            format!("{directory}/{name}")
        })
    }

    /// Rename one workspace entry within its own parent directory.
    pub fn workspace_local_rename(
        &self,
        root_path: &str,
        relative: &str,
        new_name: &str,
    ) -> Result<()> {
        validate_entry_name(new_name)?;
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let source = workspace_resource_target(&root, relative)?;
        let parent = source
            .parent()
            .ok_or_else(|| anyhow!("路径超出当前工作区"))?;
        let target = parent.join(new_name.trim());
        if paths_equal(&target, &source) {
            return Ok(());
        }
        if target.exists() {
            bail!("同名文件或目录已存在");
        }
        std::fs::rename(&source, &target).map_err(|error| anyhow!("重命名失败：{error}"))?;
        Ok(())
    }

    /// Delete one workspace entry; directories go with their whole subtree.
    /// The workspace root itself is never deletable.
    pub fn workspace_local_delete(&self, root_path: &str, relative: &str) -> Result<()> {
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let target = workspace_resource_target(&root, relative)?;
        if paths_equal(&target, &root) {
            bail!("不能删除工作区根目录");
        }
        if target.is_dir() {
            std::fs::remove_dir_all(&target).map_err(|error| anyhow!("删除失败：{error}"))?;
        } else {
            std::fs::remove_file(&target).map_err(|error| anyhow!("删除失败：{error}"))?;
        }
        Ok(())
    }

    /// Validate a new-entry relative path and resolve it under `root`. Unlike
    /// `workspace_resource_target` the entry does not exist yet, so the
    /// boundary check is purely lexical.
    fn workspace_new_target(&self, root_path: &str, relative: &str) -> Result<PathBuf> {
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        workspace_new_entry_target(&root, relative)
    }

    /// List one local workspace folder for the working-directory region. Paths
    /// are relative to the workspace root and cannot escape it; the result uses
    /// the same shape as the cloud listing so the panel projects either source
    /// identically. Mutations run through the `workspace_local_*` façade.
    pub fn workspace_local_listing(
        &self,
        root_path: &str,
        path: &str,
        offset: u64,
    ) -> Result<NativeInterlinkListing> {
        const LOCAL_PAGE: usize = 200;
        const BROWSE_CAP: usize = 2000;
        if offset >= BROWSE_CAP as u64 {
            bail!("已达到目录浏览上限");
        }
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        // The root page is the workspace folder itself; children resolve under
        // it through the same boundary check the open path uses.
        let dir = if path.is_empty() {
            root.clone()
        } else {
            workspace_resource_target(&root, path)?
        };
        if !dir.is_dir() {
            bail!("目录不存在");
        }
        // Keep only the requested prefix while scanning. Large host folders
        // must not allocate a complete directory listing just to show 200 rows.
        let keep = (offset as usize + LOCAL_PAGE).min(BROWSE_CAP);
        let mut entries: BinaryHeap<LocalListingCandidate> = BinaryHeap::with_capacity(keep);
        let mut total = 0u64;
        for item in std::fs::read_dir(&dir)? {
            let Ok(item) = item else { continue };
            let name = item.file_name().to_string_lossy().to_string();
            if name.is_empty() {
                continue;
            }
            let Ok(kind) = item.file_type() else { continue };
            let is_dir = kind.is_dir();
            total += 1;
            let candidate = LocalListingCandidate(NativeInterlinkEntry {
                name: name.clone(),
                path: if path.is_empty() {
                    name
                } else {
                    format!("{path}/{name}")
                },
                kind: if is_dir { "dir".into() } else { "file".into() },
                size: 0,
                updated_time: String::new(),
            });
            if entries.len() < keep {
                entries.push(candidate);
            } else if entries.peek().is_some_and(|last| candidate < *last) {
                entries.pop();
                entries.push(candidate);
            }
        }
        let page = entries
            .into_sorted_vec()
            .into_iter()
            .map(|candidate| candidate.0)
            .skip(offset as usize)
            .take(LOCAL_PAGE)
            .map(|mut entry| {
                entry.size = std::fs::metadata(dir.join(&entry.name))
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                entry
            })
            .collect();
        Ok(NativeInterlinkListing {
            status: "succeeded".into(),
            path: path.to_string(),
            entries: page,
            total,
            error: None,
            command_id: None,
        })
    }

    /// Filename search over one local workspace folder for the working-
    /// directory region: a bounded depth-first walk matching the keyword as a
    /// case-insensitive substring of entry names. The result reuses the cloud
    /// listing shape, so the panel projects both sources identically.
    pub fn workspace_local_search(
        &self,
        root_path: &str,
        keyword: &str,
    ) -> Result<NativeInterlinkListing> {
        const SEARCH_RESULTS: usize = 200;
        const SEARCH_VISITED: usize = 20_000;
        const SEARCH_DEPTH: usize = 12;
        let keyword = keyword.trim();
        anyhow::ensure!(!keyword.is_empty(), "关键词为空");
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let needle: String = keyword.chars().flat_map(char::to_lowercase).collect();
        let mut matches: Vec<NativeInterlinkEntry> = Vec::new();
        let mut visited = 0usize;
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(root, String::new(), 0)];
        'walk: while let Some((dir, prefix, depth)) = stack.pop() {
            let Ok(items) = std::fs::read_dir(&dir) else {
                continue;
            };
            for item in items.flatten() {
                visited += 1;
                if visited >= SEARCH_VISITED || matches.len() >= SEARCH_RESULTS {
                    break 'walk;
                }
                let name = item.file_name().to_string_lossy().to_string();
                if name.is_empty() || name.starts_with('.') {
                    continue;
                }
                let Ok(kind) = item.file_type() else { continue };
                let is_dir = kind.is_dir();
                let hay: String = name.chars().flat_map(char::to_lowercase).collect();
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if hay.contains(&needle) {
                    let size = if is_dir {
                        0
                    } else {
                        item.metadata().map(|meta| meta.len()).unwrap_or(0)
                    };
                    matches.push(NativeInterlinkEntry {
                        name: name.clone(),
                        path: path.clone(),
                        kind: if is_dir { "dir".into() } else { "file".into() },
                        size,
                        updated_time: String::new(),
                    });
                }
                if is_dir && depth + 1 < SEARCH_DEPTH {
                    stack.push((item.path(), path, depth + 1));
                }
            }
        }
        matches.sort_by(|a, b| {
            (b.kind == "dir")
                .cmp(&(a.kind == "dir"))
                .then_with(|| natural_name_cmp(&a.name, &b.name))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(NativeInterlinkListing {
            status: "succeeded".into(),
            path: String::new(),
            total: matches.len() as u64,
            entries: matches,
            error: None,
            command_id: None,
        })
    }

    /// Copy one workspace entry — or, for the root, every child entry — out of
    /// the workspace into a user-picked destination folder. This is the local
    /// working directory's download: a plain host copy, no archive detour.
    /// Existing entries in the destination are replaced, matching the upload
    /// semantics. Returns the number of top-level entries copied.
    pub fn workspace_local_export(
        &self,
        root_path: &str,
        relative: &str,
        destination: &str,
    ) -> Result<u64> {
        let root = normalize_root(root_path)?;
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let destination = PathBuf::from(destination);
        if !destination.is_dir() {
            bail!("目标文件夹不存在");
        }
        if relative.is_empty() {
            let mut copied = 0u64;
            for item in std::fs::read_dir(&root)? {
                let item = item?;
                if item.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                copy_entry_out(&item.path(), &destination.join(item.file_name()))?;
                copied += 1;
            }
            return Ok(copied);
        }
        let source = workspace_resource_target(&root, relative)?;
        if !source.exists() {
            bail!("文件不存在");
        }
        let name = source
            .file_name()
            .ok_or_else(|| anyhow!("无效的路径"))?
            .to_owned();
        copy_entry_out(&source, &destination.join(name))?;
        Ok(1)
    }

    /// Read one workspace image for Markdown rendering after enforcing the
    /// workspace path boundary. The UI receives bounded bytes instead of a
    /// host path, so message content cannot become a local-file reader.
    pub fn workspace_image_bytes(&self, root_path: &str, relative: &str) -> Result<Vec<u8>> {
        const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
        let root = PathBuf::from(root_path);
        // Chat attachments are persisted in the private user workspace, while
        // their message-facing path uses the public `/workspaces/<scope>/…`
        // namespace (or, in the local single-root form, an absolute path
        // inside the private workspace).  The active thread can belong to a
        // user-selected workspace, so resolving that path against `root`
        // would point at the wrong folder and leave the image stuck at
        // "暂未加载".  Only the exact private public root / private absolute
        // root and `chat_attachments/` prefix are remapped; all other sources
        // keep the regular workspace containment check below.
        let target = if let Some(target) = self.chat_attachment_target(relative)? {
            target
        } else {
            if !root.is_dir() {
                bail!(WORKSPACE_ROOT_MISSING);
            }
            workspace_resource_target(&root, relative)?
        };
        let metadata = target.metadata()?;
        if !metadata.is_file() {
            bail!("请选择工作目录中的图片文件");
        }
        if metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            bail!("图片大小超出 16 MiB 展示上限");
        }
        let extension = target
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        // These are the formats decoded by the desktop image feature. Keeping
        // this list explicit avoids a UI promise that the native renderer
        // cannot fulfil on the older desktop targets.
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
            bail!("此图片格式暂不支持");
        }
        let mut bytes = Vec::new();
        std::fs::File::open(target)?
            .take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            bail!("图片大小超出展示上限");
        }
        Ok(bytes)
    }

    /// Resolve a persisted chat attachment from its public workspace path to
    /// the controlled private user workspace.  Returning `None` means that
    /// the source is an ordinary workspace-relative image and should be
    /// resolved against the caller-provided workspace root.
    fn chat_attachment_target(&self, source: &str) -> Result<Option<PathBuf>> {
        let private_scope = self
            .state()
            .workspace
            .scoped_user_id_by_container(self.user_id(), USER_PRIVATE_CONTAINER_ID);
        let public_root = self.state().workspace.public_root(&private_scope);
        let private_root = self.state().workspace.workspace_root(&private_scope);
        if let Some(relative) = chat_attachment_relative(source, &public_root) {
            if !private_root.is_dir() {
                bail!(WORKSPACE_ROOT_MISSING);
            }
            return Ok(Some(workspace_resource_target(&private_root, &relative)?));
        }
        // 本地单根形态下新附件的消息路径是私有工作区内的绝对路径（见
        // WorkspaceManager::display_path 的单根分支）。同样只放行
        // chat_attachments/ 前缀，避免消息内容变成任意文件读取器。
        let decoded = percent_decode_path(source)?.replace('\\', "/");
        let candidate = PathBuf::from(decoded);
        if !candidate.is_absolute() {
            return Ok(None);
        }
        let Ok(canonical) = candidate.canonicalize() else {
            return Ok(None);
        };
        let root_canonical = private_root.canonicalize().unwrap_or(private_root);
        let Ok(relative) = canonical.strip_prefix(&root_canonical) else {
            return Ok(None);
        };
        if !relative
            .to_string_lossy()
            .replace('\\', "/")
            .starts_with("chat_attachments/")
        {
            return Ok(None);
        }
        Ok(Some(canonical))
    }
}

fn native_workspace(record: &WorkspaceRecord, thread_count: i64) -> NativeWorkspace {
    NativeWorkspace {
        workspace_id: record.workspace_id.clone(),
        name: record.name.clone(),
        root_path: record.root_path.clone(),
        icon: record.icon.clone(),
        color: record.color.clone(),
        sort_index: record.sort_index,
        created_at: record.created_at,
        updated_at: record.updated_at,
        thread_count,
    }
}

fn normalize_choice(input: &str, fallback: &str, allowed: &[&str]) -> String {
    let value = input.trim();
    if allowed.contains(&value) {
        value.to_string()
    } else {
        fallback.to_string()
    }
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Canonicalize a candidate workspace folder; on Windows canonical paths carry
/// a verbatim prefix that is stripped for stable storage and display.
fn normalize_root(raw: &str) -> Result<PathBuf> {
    if raw.trim().is_empty() {
        bail!("路径为空");
    }
    let path = wunder_server::local_bootstrap::normalize_local_workspace_path(Path::new(raw));
    if !path.exists() {
        bail!(WORKSPACE_ROOT_MISSING);
    }
    Ok(path)
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    wunder_server::local_bootstrap::local_workspace_identity_key(a)
        == wunder_server::local_bootstrap::local_workspace_identity_key(b)
}

fn probe_writable(root: &Path) -> bool {
    let probe = root.join(format!(".wunder-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Canonical form used for the paths this module returns: symlinks and 8.3
/// short names are resolved and the Windows `\\?\` verbatim prefix is
/// stripped, matching what a workspace root goes through in `normalize_root`.
/// Keeping one form on both sides is what a raw `canonicalize()` broke: it
/// returned `\\?\C:\work\file` against a root of `C:\work`, so every nested
/// entry looked like it escaped the workspace.
fn workspace_canonical(path: &Path) -> PathBuf {
    wunder_server::local_bootstrap::normalize_local_workspace_path(path)
}

/// Single containment decision for workspace entries. `is_within_root` already
/// resolves symlinks, tolerates a not-yet-existing leaf through its nearest
/// existing ancestor and folds Windows casing and verbatim prefixes, so callers
/// must not reimplement the comparison with `starts_with`.
fn workspace_contains(root: &Path, target: &Path) -> bool {
    wunder_server::path_utils::is_within_root(root, target)
}

/// Resolve a workspace-relative resource path (Markdown image source) to a
/// canonical file inside `root`. Rejects parent traversal, absolute paths
/// outside the root and remote URLs.
fn workspace_resource_target(root: &Path, source: &str) -> Result<PathBuf> {
    let root = workspace_canonical(root);
    let source = source.strip_prefix("\\\\?\\").unwrap_or(source);
    if source.starts_with("\\\\") || source.starts_with("//") {
        bail!("不支持远程图片路径");
    }
    let source = source.split(['?', '#']).next().unwrap_or_default();
    let path = if source.starts_with("file:") {
        let url = url::Url::parse(source)?;
        if url.host_str().is_some_and(|host| host != "localhost") {
            bail!("不支持远程图片路径");
        }
        let target = url
            .to_file_path()
            .map_err(|_| anyhow!("无效本地图片路径"))?;
        // Both sides go through the same canonical form: the URL target is
        // plain while the root may arrive verbatim or plain, and a verbatim
        // target never prefix-matches a plain root.
        let target = workspace_canonical(&target);
        if !workspace_contains(&root, &target) {
            bail!("路径超出当前工作区");
        }
        return Ok(target);
    } else {
        // Decode URL-escaped relative filenames before enforcing the boundary.
        let decoded = percent_decode_path(source)?.replace('\\', "/");
        PathBuf::from(normalize_public_workspace_source(&decoded, &root))
    };
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        bail!("路径超出当前工作区");
    }
    if path.to_string_lossy().trim().is_empty() {
        bail!("无效的工作区路径");
    }
    // Resolve the entry (it must exist), then hand back the plain canonical
    // form so downstream callers never see a `\\?\` prefix. The containment
    // test tolerates either spelling, but resolving first is what keeps a
    // bogus or escaping source from being reported as "inside".
    let target = workspace_canonical(&root.join(&path).canonicalize()?);
    if !workspace_contains(&root, &target) {
        bail!("路径超出当前工作区");
    }
    Ok(target)
}

/// Return the workspace-relative attachment path when `source` is one of the
/// public paths emitted for the current user's private workspace.  Matching is
/// deliberately exact: a message cannot use this shortcut to read another
/// scope or an arbitrary local path.
fn chat_attachment_relative(source: &str, public_root: &Path) -> Option<String> {
    let source = source.strip_prefix("\\\\?\\").unwrap_or(source);
    let source = source.split(['?', '#']).next().unwrap_or_default();
    if source.starts_with("file:") || source.starts_with("\\\\") || source.starts_with("//") {
        return None;
    }
    let decoded = percent_decode_path(source).ok()?.replace('\\', "/");
    let source = decoded.trim_start_matches('/');
    let public_root = public_root
        .to_string_lossy()
        .replace('\\', "/")
        .trim_matches('/')
        .to_string();
    let remainder = source.strip_prefix(&public_root)?;
    let remainder = remainder.strip_prefix('/')?;
    if !remainder.starts_with("chat_attachments/") {
        return None;
    }
    Some(remainder.to_string())
}

fn percent_decode_path(source: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(source.len());
    let raw = source.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            if let (Some(a), Some(b)) = (
                (raw[index + 1] as char).to_digit(16),
                (raw[index + 2] as char).to_digit(16),
            ) {
                bytes.push((a * 16 + b) as u8);
                index += 3;
                continue;
            }
        }
        bytes.push(raw[index]);
        index += 1;
    }
    Ok(String::from_utf8(bytes)?)
}

/// Convert the logical paths emitted by cloud agents back to the local
/// workspace-relative form before applying the normal containment check.
/// Accepted forms:
/// - `/workspaces/<relative>` — local single-root form (CLI/desktop), where
///   the segment after `workspaces/` is already part of the relative path;
/// - `/workspaces/<scope>/file` and the cloud form
///   `/workspaces/<user>/workspace/file`.
/// The single-root interpretation wins whenever the whole remainder resolves
/// to an existing entry inside `root`; otherwise the first segment is treated
/// as a scope and stripped.
fn normalize_public_workspace_source(source: &str, root: &Path) -> String {
    let normalized = source.trim_start_matches('/');
    let Some(rest) = normalized.strip_prefix("workspaces/") else {
        return source.to_string();
    };
    if !rest.trim().is_empty() && root.join(rest).exists() {
        return rest.to_string();
    }
    let mut parts = rest.splitn(2, '/');
    let _scope = parts.next().unwrap_or_default();
    let remainder = parts.next().unwrap_or_default();
    if let Some(relative) = remainder.strip_prefix("workspace/") {
        relative.to_string()
    } else {
        remainder.to_string()
    }
}

/// Validate one path segment the user typed for a new or renamed entry.
/// Recursive copy used by the local export: files are replaced, directories
/// are merged with the same rule. The walk is bounded so a cyclic or very
/// deep tree cannot pin the worker thread.
fn copy_entry_out(source: &Path, target: &Path) -> Result<()> {
    const EXPORT_DEPTH: usize = 16;
    const EXPORT_BUDGET: usize = 20_000;
    fn run(source: &Path, target: &Path, depth: usize, budget: &mut usize) -> Result<()> {
        if *budget == 0 {
            bail!("导出内容超出上限");
        }
        *budget -= 1;
        if source.is_dir() {
            anyhow::ensure!(depth < EXPORT_DEPTH, "目录嵌套过深，无法导出");
            std::fs::create_dir_all(target).map_err(|error| anyhow!("创建目录失败：{error}"))?;
            for item in std::fs::read_dir(source)? {
                let item = item?;
                let name = item.file_name();
                if name.to_string_lossy().starts_with('.') {
                    continue;
                }
                run(&item.path(), &target.join(name), depth + 1, budget)?;
            }
            return Ok(());
        }
        std::fs::copy(source, target)
            .map(|_| ())
            .map_err(|error| anyhow!("复制失败：{error}"))
    }
    let mut budget = EXPORT_BUDGET;
    run(source, target, 0, &mut budget)
}

fn validate_entry_name(name: &str) -> Result<()> {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        bail!("名称不能为空");
    }
    if cleaned.chars().count() > 120 {
        bail!("名称过长");
    }
    if cleaned == "." || cleaned == ".." {
        bail!("无效的名称");
    }
    if cleaned.chars().any(|c| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        bail!("名称不能包含分隔符或特殊字符");
    }
    if cfg!(windows) && (cleaned.ends_with('.') || cleaned.ends_with(' ')) {
        bail!("名称不能以点或空格结尾");
    }
    Ok(())
}

/// Resolve a workspace-relative path for an entry that does not exist yet:
/// the boundary check stays lexical (no canonicalize) and parent traversal,
/// prefixes and absolute forms are rejected outright.
fn workspace_new_entry_target(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = PathBuf::from(relative.trim().replace('\\', "/"));
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("路径超出当前工作区");
    }
    if path.as_os_str().is_empty() {
        bail!("无效的工作区路径");
    }
    let target = root.join(&path);
    if !target.starts_with(root) {
        bail!("路径超出当前工作区");
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_paths_are_confined_for_relative_absolute_file_and_url_forms() {
        let directory = tempfile::tempdir().unwrap();
        // Use the same form the workspace really stores (verbatim prefix
        // stripped). A raw `canonicalize()` root hid the Windows mismatch
        // between `\\?\C:\...` and `C:\...`, so the test passed while every
        // nested entry was rejected at runtime.
        let root = wunder_server::local_bootstrap::normalize_local_workspace_path(directory.path());
        let file = root.join("sample image.png");
        std::fs::write(&file, b"fixture").unwrap();
        let file_url = url::Url::from_file_path(&file).unwrap();
        for source in [
            "./sample%20image.png",
            "sample image.png",
            file.to_str().unwrap(),
            file_url.as_str(),
        ] {
            assert_eq!(
                super::workspace_resource_target(&root, source).unwrap(),
                wunder_server::local_bootstrap::normalize_local_workspace_path(&file)
            );
        }
        for source in [
            "../outside.png",
            "%2e%2e/outside.png",
            "https://example.invalid/image.png",
        ] {
            assert!(super::workspace_resource_target(&root, source).is_err());
        }
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert!(super::workspace_resource_target(&root, outside.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn nested_entries_resolve_under_the_stored_plain_root() {
        // Regression: a freshly seeded default workspace stores a plain root
        // (`C:\...`, verbatim prefix stripped). Resolving an existing nested
        // entry used to canonicalize only the target, compare `\\?\C:\...`
        // against `C:\...` and reject it as "路径超出当前工作区".
        let directory = tempfile::tempdir().unwrap();
        let root = wunder_server::local_bootstrap::normalize_local_workspace_path(directory.path());
        let nested = root.join("notes");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("todo.md"), b"ok").unwrap();

        let dir_target = super::workspace_resource_target(&root, "notes").unwrap();
        assert!(dir_target.is_dir());
        let file_target = super::workspace_resource_target(&root, "notes/todo.md").unwrap();
        assert!(file_target.is_file());
        // A verbatim spelling still counts as inside the same workspace.
        let verbatim = nested.join("todo.md").canonicalize().unwrap();
        assert!(super::workspace_contains(&root, &verbatim));
        // Escaping entries stay rejected in both spellings.
        assert!(super::workspace_resource_target(&root, "../escape.md").is_err());
        let outside = tempfile::NamedTempFile::new().unwrap();
        assert!(!super::workspace_contains(
            &root,
            &outside.path().canonicalize().unwrap()
        ));
    }


    #[test]
    fn normalize_choice_keeps_allowed_values_and_falls_back() {
        assert_eq!(
            normalize_choice("code", "folder", &["folder", "code"]),
            "code"
        );
        assert_eq!(
            normalize_choice("nope", "folder", &["folder", "code"]),
            "folder"
        );
        assert_eq!(normalize_choice("  ", "blue", &["blue"]), "blue");
    }

    #[test]
    fn entry_names_reject_separators_and_traversal() {
        assert!(validate_entry_name(" 新目录 ").is_ok());
        for bad in [
            "", "  ", ".", "..", "a/b", "a\\b", "a:b", "a*b", "a?b", "a<b",
        ] {
            assert!(validate_entry_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn public_workspace_markdown_paths_become_workspace_relative() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        assert_eq!(
            normalize_public_workspace_source("/workspaces/demo/workspace/爱心.png", root),
            "爱心.png"
        );
        assert_eq!(
            normalize_public_workspace_source("/workspaces/demo__c__1/reports/result.pdf", root),
            "reports/result.pdf"
        );
    }

    #[test]
    fn single_root_public_paths_keep_the_whole_relative_path() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("temp")).unwrap();
        std::fs::write(root.join("temp/shot.png"), b"png").unwrap();
        // 本地单根形态：temp 是真实目录而不是用户 scope，必须整段保留。
        assert_eq!(
            normalize_public_workspace_source("/workspaces/temp/shot.png", root),
            "temp/shot.png"
        );
        // 整段不存在时仍回退到云端 scope 解释。
        assert_eq!(
            normalize_public_workspace_source("/workspaces/temp/missing.png", root),
            "missing.png"
        );
    }

    #[test]
    fn chat_attachment_paths_match_only_the_current_private_public_root() {
        let root = Path::new("/workspaces/current-user");
        assert_eq!(
            chat_attachment_relative(
                "/workspaces/current-user/chat_attachments/session/image%20one.png",
                root,
            ),
            Some("chat_attachments/session/image one.png".into())
        );
        for source in [
            "/workspaces/another-user/chat_attachments/session/image.png",
            "/workspaces/current-user/workspace/image.png",
            "file:///tmp/chat_attachments/image.png",
            "https://example.invalid/chat_attachments/image.png",
        ] {
            assert!(chat_attachment_relative(source, root).is_none(), "{source}");
        }
    }

    #[test]
    fn new_entry_targets_stay_inside_the_root() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        assert_eq!(
            workspace_new_entry_target(&root, "notes/todo.md").unwrap(),
            root.join("notes/todo.md")
        );
        for bad in ["", "  ", "../x.md", "a/../../x.md", "/abs/x.md"] {
            assert!(workspace_new_entry_target(&root, bad).is_err(), "{bad}");
        }
    }
}
