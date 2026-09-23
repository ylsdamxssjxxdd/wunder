use super::NativeDesktop;
use anyhow::{bail, Result};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
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

impl NativeDesktop {
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
        if path.contains(':')
            || path.contains('\0')
            || Path::new(path).components().any(|p| {
                matches!(
                    p,
                    Component::Prefix(_) | Component::RootDir | Component::ParentDir
                )
            })
        {
            bail!("路径超出当前工作区");
        }
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
        self.confined_path(&scope, path)?;
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
            return Ok("此文件为二进制内容，暂不提供预览。".into());
        }
        let mut text = String::from_utf8_lossy(&bytes).into_owned();
        if truncated {
            text.push_str("\n\n（仅预览前 32 KiB）");
        }
        Ok(text)
    }
}
