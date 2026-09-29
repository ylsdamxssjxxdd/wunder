use super::NativeDesktop;
use anyhow::{anyhow, bail, Result};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug)]
pub struct FileRecord {
    pub name: String,
    pub path: String,
    pub entry_type: String,
    pub size: String,
}

pub struct Directory {
    pub container_id: i32,
    pub root: String,
    pub path: String,
    pub parent: String,
    pub total: i32,
    pub entries: Vec<FileRecord>,
}

pub struct WorkspacePreview {
    pub text: String,
    pub editable: bool,
}

impl NativeDesktop {
    pub fn open_workspace_file(&self, agent: &str, path: &str) -> Result<()> {
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_path(&scope, path)?;
        if !target.exists() {
            bail!("文件不存在");
        }
        #[cfg(windows)]
        {
            Command::new("cmd")
                .args(["/C", "start", "", &target.to_string_lossy()])
                .spawn()?;
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
    pub fn create_workspace_file(&self, agent: &str, path: &str, content: &str) -> Result<()> {
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_destination(&scope, path)?;
        if target.exists() {
            bail!("文件已存在");
        }
        self.write_workspace_bytes(&scope, &target, content.as_bytes())?;
        Ok(())
    }

    /// Creates one directory below the active agent container. Existing
    /// directories are deliberately rejected so callers cannot mistake a
    /// failed create for a successful mutation.
    pub fn create_workspace_directory(&self, agent: &str, path: &str) -> Result<()> {
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_destination(&scope, path)?;
        if target.exists() {
            bail!("文件或目录已存在");
        }
        std::fs::create_dir(&target)?;
        self.state().workspace.refresh_workspace_tree(&scope);
        Ok(())
    }

    /// Replaces an existing small UTF-8 text file. The size cap prevents a
    /// native editor save from turning the UI projection into an unbounded
    /// file transport.
    pub fn save_workspace_text(&self, agent: &str, path: &str, content: &str) -> Result<()> {
        const MAX_EDIT_BYTES: usize = 1024 * 1024;
        if content.len() > MAX_EDIT_BYTES {
            bail!("文本超过 1 MiB 编辑上限");
        }
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_path(&scope, path)?;
        if !target.metadata()?.is_file() {
            bail!("请选择普通文件");
        }
        let preview = self.workspace_preview_detail(agent, path)?;
        if !preview.editable {
            bail!("仅完整 UTF-8 文本预览可编辑，请使用系统程序编辑此文件");
        }
        self.write_workspace_bytes(&scope, &target, content.as_bytes())
    }

    /// Imports caller-owned bytes without exposing a host path to the runtime.
    pub fn upload_workspace_bytes(&self, agent: &str, path: &str, bytes: &[u8]) -> Result<()> {
        const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;
        if bytes.len() > MAX_UPLOAD_BYTES {
            bail!("文件超过 32 MiB 上传上限");
        }
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_destination(&scope, path)?;
        if target.exists() {
            bail!("文件已存在");
        }
        self.write_workspace_bytes(&scope, &target, bytes)
    }

    /// Returns a bounded download payload. Native callers choose the save
    /// location themselves; this façade never accepts an arbitrary host path.
    pub fn download_workspace_bytes(&self, agent: &str, path: &str) -> Result<Vec<u8>> {
        const MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_path(&scope, path)?;
        let metadata = target.metadata()?;
        if !metadata.is_file() {
            bail!("请选择普通文件");
        }
        if metadata.len() > MAX_DOWNLOAD_BYTES {
            bail!("文件超过 32 MiB 下载上限");
        }
        Ok(std::fs::read(target)?)
    }

    pub fn delete_workspace_entry(&self, agent: &str, path: &str) -> Result<()> {
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_path(&scope, path)?;
        self.reject_workspace_root(&scope, &target)?;
        if target.is_dir() {
            std::fs::remove_dir_all(&target)?;
        } else {
            std::fs::remove_file(&target)?;
        }
        self.state().workspace.refresh_workspace_tree(&scope);
        Ok(())
    }

    pub fn rename_workspace_entry(&self, agent: &str, path: &str, name: &str) -> Result<String> {
        Self::validate_leaf_name(name)?;
        let scope = self.workspace_scope(agent)?;
        let source = self.confined_path(&scope, path)?;
        self.reject_workspace_root(&scope, &source)?;
        let parent = source.parent().ok_or_else(|| anyhow!("无效文件路径"))?;
        let target = parent.join(name);
        if target.exists() {
            bail!("目标文件或目录已存在");
        }
        std::fs::rename(&source, &target)?;
        self.state().workspace.refresh_workspace_tree(&scope);
        Ok(Self::relative_path(&scope, &target, self)?)
    }

    pub fn move_workspace_entry(&self, agent: &str, source: &str, destination: &str) -> Result<()> {
        let scope = self.workspace_scope(agent)?;
        let source = self.confined_path(&scope, source)?;
        self.reject_workspace_root(&scope, &source)?;
        let destination = self.confined_destination(&scope, destination)?;
        if destination.starts_with(&source) {
            bail!("不能移动到自身目录中");
        }
        if destination.exists() {
            bail!("目标文件或目录已存在");
        }
        std::fs::rename(source, destination)?;
        self.state().workspace.refresh_workspace_tree(&scope);
        Ok(())
    }

    pub fn copy_workspace_entry(&self, agent: &str, source: &str, destination: &str) -> Result<()> {
        let scope = self.workspace_scope(agent)?;
        let source = self.confined_path(&scope, source)?;
        self.reject_workspace_root(&scope, &source)?;
        let destination = self.confined_destination(&scope, destination)?;
        if destination.starts_with(&source) {
            bail!("不能复制到自身目录中");
        }
        if destination.exists() {
            bail!("目标文件或目录已存在");
        }
        Self::copy_entry(&source, &destination)?;
        self.state().workspace.refresh_workspace_tree(&scope);
        Ok(())
    }
    fn workspace_scope(&self, agent: &str) -> Result<String> {
        let record = self
            .runtime
            .block_on(wunder_server::agent_management::owned(
                self.state(),
                self.user_id(),
                agent,
            ))?;
        Ok(self
            .state()
            .workspace
            .scoped_user_id_by_container(self.user_id(), record.sandbox_container_id))
    }

    fn confined_path(&self, scope: &str, path: &str) -> Result<PathBuf> {
        // WorkspaceManager permits absolute tool paths. The file browser only
        // accepts paths inside the selected agent's workspace, including symlinks.
        Self::validate_relative_path(path)?;
        let root = self
            .state()
            .workspace
            .ensure_user_root(scope)?
            .canonicalize()?;
        let target = root.join(path).canonicalize()?;
        if !target.starts_with(&root) {
            bail!("路径超出当前工作区");
        }
        Ok(target)
    }

    fn confined_destination(&self, scope: &str, path: &str) -> Result<PathBuf> {
        Self::validate_relative_path(path)?;
        let root = self
            .state()
            .workspace
            .ensure_user_root(scope)?
            .canonicalize()?;
        let target = root.join(path);
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("无效文件路径"))?
            .canonicalize()?;
        if !parent.starts_with(&root) {
            bail!("路径超出当前工作区");
        }
        let name = target.file_name().ok_or_else(|| anyhow!("无效文件路径"))?;
        Self::validate_leaf_name(&name.to_string_lossy())?;
        Ok(parent.join(name))
    }

    fn reject_workspace_root(&self, scope: &str, target: &Path) -> Result<()> {
        let root = self
            .state()
            .workspace
            .ensure_user_root(scope)?
            .canonicalize()?;
        if target == root || !target.starts_with(&root) {
            bail!("不能修改工作区根目录");
        }
        Ok(())
    }

    fn validate_relative_path(path: &str) -> Result<()> {
        if path.trim().is_empty()
            || path.contains(':')
            || path.contains('\0')
            || Path::new(path).components().any(|part| {
                matches!(
                    part,
                    Component::Prefix(_) | Component::RootDir | Component::ParentDir
                )
            })
        {
            bail!("路径超出当前工作区");
        }
        Ok(())
    }

    fn validate_leaf_name(name: &str) -> Result<()> {
        Self::validate_relative_path(name)?;
        if name.trim().is_empty()
            || Path::new(name).components().count() != 1
            || Path::new(name)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            bail!("文件名无效");
        }
        Ok(())
    }

    fn write_workspace_bytes(&self, scope: &str, target: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = std::fs::File::create(target)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        self.state().workspace.refresh_workspace_tree(scope);
        Ok(())
    }

    fn copy_entry(source: &Path, destination: &Path) -> Result<()> {
        let metadata = std::fs::symlink_metadata(source)?;
        if metadata.file_type().is_symlink() {
            bail!("不能复制符号链接");
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                bail!("不能复制重解析点");
            }
        }
        if source.is_dir() {
            std::fs::create_dir(destination)?;
            for child in std::fs::read_dir(source)? {
                let child = child?;
                Self::copy_entry(&child.path(), &destination.join(child.file_name()))?;
            }
        } else {
            std::fs::copy(source, destination)?;
        }
        Ok(())
    }

    fn relative_path(scope: &str, target: &Path, desktop: &Self) -> Result<String> {
        let root = desktop
            .state()
            .workspace
            .ensure_user_root(scope)?
            .canonicalize()?;
        Ok(target
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/"))
    }

    pub fn workspace_directory(&self, agent: &str, path: &str, offset: i32) -> Result<Directory> {
        let record = self
            .runtime
            .block_on(wunder_server::agent_management::owned(
                self.state(),
                self.user_id(),
                agent,
            ))?;
        let container_id = record.sandbox_container_id;
        let scope = self
            .state()
            .workspace
            .scoped_user_id_by_container(self.user_id(), container_id);
        let root = self
            .state()
            .workspace
            .ensure_user_root(&scope)?
            .display()
            .to_string();
        self.confined_path(&scope, if path.is_empty() { "." } else { path })?;
        let (entries, _, path, parent, total) = self.state().workspace.list_workspace_entries(
            &scope,
            if path.is_empty() { "." } else { path },
            None,
            offset.max(0) as u64,
            100,
            "name",
            "asc",
        )?;
        Ok(Directory {
            container_id,
            root,
            path,
            parent: parent.unwrap_or_default(),
            total: total.min(i32::MAX as u64) as i32,
            entries: entries
                .into_iter()
                .map(|entry| FileRecord {
                    name: entry.name,
                    path: entry.path,
                    size: if entry.entry_type == "dir" {
                        String::new()
                    } else {
                        format!("{} B", entry.size)
                    },
                    entry_type: entry.entry_type,
                })
                .collect(),
        })
    }

    pub fn workspace_preview(&self, agent: &str, path: &str) -> Result<String> {
        Ok(self.workspace_preview_detail(agent, path)?.text)
    }

    pub fn workspace_preview_detail(&self, agent: &str, path: &str) -> Result<WorkspacePreview> {
        let scope = self.workspace_scope(agent)?;
        let target = self.confined_path(&scope, path)?;
        let file = std::fs::File::open(target)?;
        if !file.metadata()?.is_file() {
            bail!("请选择普通文件");
        }
        let mut bytes = Vec::with_capacity(32_769);
        file.take(32_769).read_to_end(&mut bytes)?;
        let truncated = bytes.len() > 32_768;
        bytes.truncate(32_768);
        if bytes.contains(&0) {
            return Ok(WorkspacePreview {
                text: "此文件为二进制内容，暂不提供预览。".into(),
                editable: false,
            });
        }
        let editable = !truncated && std::str::from_utf8(&bytes).is_ok();
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if truncated {
            text.push_str("\n\n（仅预览前 32 KiB）");
        }
        Ok(WorkspacePreview { text, editable })
    }
}

#[cfg(test)]
mod tests {
    use super::NativeDesktop;
    use std::path::Path;

    #[test]
    fn relative_path_rejects_escape_and_absolute_forms() {
        for path in [
            "",
            "../outside",
            "a/../../outside",
            "/tmp/outside",
            "C:\\outside",
            "a\0b",
        ] {
            assert!(
                NativeDesktop::validate_relative_path(path).is_err(),
                "{path:?}"
            );
        }
        assert!(NativeDesktop::validate_relative_path("folder/file.txt").is_ok());
    }

    #[test]
    fn leaf_name_rejects_nested_or_empty_names() {
        assert!(NativeDesktop::validate_leaf_name("renamed.txt").is_ok());
        assert!(NativeDesktop::validate_leaf_name("").is_err());
        assert!(NativeDesktop::validate_leaf_name("../escape").is_err());
        assert!(NativeDesktop::validate_leaf_name("folder/name").is_err());
        assert!(NativeDesktop::validate_leaf_name(".").is_err());
    }

    #[test]
    fn copy_entry_handles_files() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.txt");
        let target = root.path().join("target.txt");
        std::fs::write(&source, b"content").unwrap();
        NativeDesktop::copy_entry(Path::new(&source), Path::new(&target)).unwrap();
        assert_eq!(std::fs::read(target).unwrap(), b"content");
    }
}
