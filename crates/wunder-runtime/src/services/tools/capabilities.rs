//! 能力 seam（capability seam）。
//!
//! 内置工具不应直接耦合到具体的宿主实现（本地文件系统、沙箱、远程执行）。
//! 这里抽象出最小的能力接口，由宿主提供实现；工具只依赖 trait。当前默认实现
//! 是本地实现，后续接入沙箱 / 浏览器 / 远程后端时替换实现即可，不改动工具逻辑。

use anyhow::{anyhow, Result};
use std::path::Path;

/// 文件系统能力：文本读写与目录创建。
pub(crate) trait FsCapability: Send + Sync {
    fn read_text(&self, path: &Path) -> Result<String>;
    fn write_text(&self, path: &Path, content: &str) -> Result<()>;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
}

/// 本地文件系统实现（原子写入）。
pub(crate) struct LocalFs;

impl FsCapability for LocalFs {
    fn read_text(&self, path: &Path) -> Result<String> {
        std::fs::read_to_string(path).map_err(|err| anyhow!("读取文件失败：{err}"))
    }

    fn write_text(&self, path: &Path, content: &str) -> Result<()> {
        crate::core::atomic_write::atomic_write_text(path, content)
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).map_err(|err| anyhow!("创建目录失败：{err}"))
    }
}

/// 命令执行能力 seam。当前仅暴露标识，命令类工具仍走既有 command_tool 管线；
/// 这里为后续「沙箱 / 远程 / 浏览器」三类执行后端预留统一入口。
pub(crate) trait CommandCapability: Send + Sync {
    fn kind(&self) -> &'static str;
}

/// 本地 shell 执行实现。
pub(crate) struct LocalShell;

impl CommandCapability for LocalShell {
    fn kind(&self) -> &'static str {
        "local"
    }
}

/// 返回默认的本地文件系统能力实现。
pub(crate) fn local_fs() -> LocalFs {
    LocalFs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn local_fs_round_trips_text() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("nested").join("a.txt");
        let fs_cap = local_fs();
        fs_cap
            .create_dir_all(target.parent().expect("parent"))
            .expect("mkdir");
        fs_cap.write_text(&target, "hello").expect("write");
        assert_eq!(fs_cap.read_text(&target).expect("read"), "hello");
        assert!(fs::read_to_string(&target).is_ok());
    }

    #[test]
    fn local_shell_reports_kind() {
        let shell = LocalShell;
        assert_eq!(shell.kind(), "local");
    }
}
