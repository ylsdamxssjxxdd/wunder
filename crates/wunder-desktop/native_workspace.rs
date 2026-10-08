//! Workspace façade: bind threads to real user folders.
//!
//! Workspaces are the desktop's only organizational unit. Every thread lives
//! in exactly one workspace and its tool execution root is the bound folder,
//! so all file work happens directly in the user's project directory. This
//! layer owns CRUD, path validation and the one-time local migration; storage
//! and execution semantics stay in the shared engine.

use super::NativeDesktop;
use anyhow::{anyhow, bail, Result};
use std::{
    collections::HashMap,
    io::Read,
    path::{Component, Path, PathBuf},
};
use wunder_server::storage::WorkspaceRecord;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::Command;

pub const WORKSPACE_ROOT_MISSING: &str = "工作区文件夹不存在或不可访问";

/// Icon choices offered by the workspace dialog; stored values outside this
/// set fall back to the default icon at read time.
pub const WORKSPACE_ICONS: &[&str] = &[
    "folder", "coffee", "filter", "cake", "moon", "knight", "robot", "flower", "gear", "swan",
    "bear", "ghost", "alien", "bell", "flask",
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
        Ok(native_workspace(&record, 0))
    }

    pub fn update_workspace(&self, edit: &WorkspaceEdit) -> Result<NativeWorkspace> {
        let record = self.store_workspace(edit)?;
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
        Ok(summary)
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
            let other = PathBuf::from(&record.root_path);
            if paths_equal(&other, &root) {
                report.occupied_by = Some(record.name);
            } else if other.starts_with(&root) {
                report.contains = Some(record.name);
            } else if root.starts_with(&other) {
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
            let other = PathBuf::from(&record.root_path);
            if paths_equal(&other, &root) {
                bail!("该文件夹已被工作区「{}」使用", record.name);
            }
            if other.starts_with(&root) || root.starts_with(&other) {
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
        let mut offset = 0i64;
        const PAGE: i64 = 200;
        loop {
            let (records, total) = storage.list_chat_sessions_by_workspace(
                self.user_id(),
                workspace_id,
                Some("active"),
                offset,
                PAGE,
            )?;
            let page = records.len();
            for mut record in records {
                record.status = "archived".into();
                record.updated_at = now_ts();
                storage.upsert_chat_session(&record)?;
                archived += 1;
            }
            offset += page as i64;
            if page == 0 || offset >= total {
                break;
            }
        }
        Ok(archived)
    }

    fn discard_workspace_threads(&self, workspace_id: &str) -> Result<usize> {
        let storage = self.state().storage.as_ref();
        let mut deleted = 0;
        let mut offset = 0i64;
        const PAGE: i64 = 200;
        loop {
            let (records, total) = storage.list_chat_sessions_by_workspace(
                self.user_id(),
                workspace_id,
                None,
                offset,
                PAGE,
            )?;
            let page = records.len();
            for record in records {
                storage.delete_chat_session(self.user_id(), &record.session_id)?;
                deleted += 1;
            }
            offset += page as i64;
            if page == 0 || offset >= total {
                break;
            }
        }
        Ok(deleted)
    }

    /// Open one workspace file with the OS default application. Paths are
    /// relative to the workspace root and cannot escape it.
    pub fn open_workspace_resource(&self, root_path: &str, relative: &str) -> Result<()> {
        let root = PathBuf::from(root_path);
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let target = workspace_resource_target(&root, relative)?;
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

    /// Read one workspace image for Markdown rendering after enforcing the
    /// workspace path boundary. The UI receives bounded bytes instead of a
    /// host path, so message content cannot become a local-file reader.
    pub fn workspace_image_bytes(&self, root_path: &str, relative: &str) -> Result<Vec<u8>> {
        const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
        let root = PathBuf::from(root_path);
        if !root.is_dir() {
            bail!(WORKSPACE_ROOT_MISSING);
        }
        let target = workspace_resource_target(&root, relative)?;
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
        // These are the formats compiled into Slint's native software
        // renderer. Keeping this list explicit avoids a UI promise that the
        // local renderer cannot fulfil on the older desktop targets.
        if !matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
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
    let path = raw.strip_prefix("\\\\?\\").unwrap_or(raw);
    if path.trim().is_empty() {
        bail!("路径为空");
    }
    let canonical = Path::new(path).canonicalize()?;
    Ok(canonical)
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
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

/// Resolve a workspace-relative resource path (Markdown image source) to a
/// canonical file inside `root`. Rejects parent traversal, absolute paths
/// outside the root and remote URLs.
fn workspace_resource_target(root: &Path, source: &str) -> Result<PathBuf> {
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
        // Root comes in canonicalized; the URL target is plain, so align the
        // two before the containment check (verbatim `\\?\` prefixes differ).
        let target = target.canonicalize().unwrap_or(target);
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if !target.starts_with(&root) {
            bail!("路径超出当前工作区");
        }
        return Ok(target);
    } else {
        // Decode URL-escaped relative filenames before enforcing the boundary.
        let mut bytes = Vec::new();
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
        PathBuf::from(String::from_utf8(bytes)?.replace('\\', "/"))
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
    let target = root.join(&path).canonicalize()?;
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
        let root = directory.path().canonicalize().unwrap();
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
                file.canonicalize().unwrap()
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
}
