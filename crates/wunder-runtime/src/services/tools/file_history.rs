//! 写入前自动快照：每次覆盖/编辑前把旧内容留一份，坏了能 diff 回滚。
//!
//! 存放位置固定在工作区 `temp/.history/` 下，按目标文件路径镜像，文件名追加
//! `@序号`：`temp/pelican.py` 的第 3 版是 `temp/.history/pelican.py@3`。这样
//! 快照路径本身就能被 `read_file` / `edit` 直接使用，也不会污染用户的项目目录。
//!
//! 约束：
//! - 只读旧内容、只写快照，失败一律返回 `None`，绝不阻断真正的写入；
//! - 每个文件最多保留 [`KEEP_PER_FILE`] 版，按序号从旧到新淘汰；
//! - 超过 [`MAX_SNAPSHOT_BYTES`] 的大文件跳过，避免把仓库撑爆；
//! - `WUNDER_FILE_HISTORY=0` 可整体关掉。

use crate::core::atomic_write::atomic_write_text;
use std::fs;
use std::path::{Path, PathBuf};

/// 单个快照的内容上限；超过就跳过（只丢历史，不影响写入）。
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
/// 每个目标文件保留的版本数。
const KEEP_PER_FILE: u32 = 20;
/// 工作区内的历史根目录（相对工作区根）。
const HISTORY_DIR_REL: &str = "temp/.history";
/// 工作区外文件的快照放在这个子目录下。
const EXTERNAL_DIR: &str = "_external";
/// 外部路径压缩后的最大长度。
const MAX_EXTERNAL_NAME: usize = 80;

/// 一次成功的快照：`display` 是可直接读写的相对路径。
#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub display: String,
    pub rev: u32,
    pub bytes: usize,
}

/// 全局开关：`WUNDER_FILE_HISTORY=0/off` 关闭快照。
pub(crate) fn enabled() -> bool {
    match std::env::var("WUNDER_FILE_HISTORY") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    }
}

/// 快照根目录（会按需创建）。
pub(crate) fn history_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(HISTORY_DIR_REL)
}

/// 覆盖/编辑前调用：把 `previous` 存成新版本，返回可回读的路径。
///
/// 任何 IO 失败都只返回 `None`：快照是增强能力，不是写入的前置条件。
pub(crate) fn snapshot(
    target: &Path,
    workspace_root: &Path,
    previous: &str,
) -> Option<Snapshot> {
    if !enabled() || previous.len() > MAX_SNAPSHOT_BYTES {
        return None;
    }
    let root = history_root(workspace_root);
    let mirrored = mirror_path(target, workspace_root);
    let (dir_part, name) = split_mirror(&mirrored)?;
    let dir = if dir_part.is_empty() {
        root.clone()
    } else {
        root.join(&dir_part)
    };
    fs::create_dir_all(&dir).ok()?;
    let rev = next_rev(&dir, &name)?;
    let entry = dir.join(entry_name(&name, rev));
    atomic_write_text(&entry, previous).ok()?;
    prune(&dir, &name, rev);
    let display = workspace_relative(&entry, workspace_root);
    Some(Snapshot {
        display,
        rev,
        bytes: previous.len(),
    })
}

/// 目标路径 → 镜像相对路径；工作区外统一塞进 `_external/`。
fn mirror_path(target: &Path, workspace_root: &Path) -> String {
    let absolute = target.to_string_lossy().replace('\\', "/");
    let root = workspace_root.to_string_lossy().replace('\\', "/");
    let root = root.trim_end_matches('/');
    if let Some(rest) = absolute.strip_prefix(root) {
        let rest = rest.trim_start_matches('/');
        if !rest.is_empty() {
            // 历史根就在 `temp/.history`，镜像路径去掉固定的 `temp/` 前缀：
            // `temp/pelican.py` 的第 3 版 = `temp/.history/pelican.py@3`。
            return rest.trim_start_matches("temp/").to_string();
        }
    }
    let mut safe: String = absolute
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if safe.len() > MAX_EXTERNAL_NAME {
        safe = safe
            .chars()
            .rev()
            .take(MAX_EXTERNAL_NAME)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
    }
    format!("/{}/{}", EXTERNAL_DIR, safe)
}

fn split_mirror(mirrored: &str) -> Option<(String, String)> {
    let path = Path::new(mirrored);
    let name = path.file_name()?.to_str()?.to_string();
    let dir_part = path
        .parent()
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let dir_part = dir_part.trim_matches('/').to_string();
    Some((dir_part, name))
}

fn entry_name(name: &str, rev: u32) -> String {
    format!("{}@{}", name, rev)
}

/// 目标文件的最新快照：`command=undo_edit` 靠它一步退回去。
///
/// 找不到历史（从未改写过、历史关闭、或快照已被淘汰）返回 `None`，
/// 由调用方决定是提示还是退化为普通写入。
pub(crate) fn latest(target: &Path, workspace_root: &Path) -> Option<Snapshot> {
    if !enabled() {
        return None;
    }
    let root = history_root(workspace_root);
    let mirrored = mirror_path(target, workspace_root);
    let (dir_part, name) = split_mirror(&mirrored)?;
    let dir = if dir_part.is_empty() {
        root.clone()
    } else {
        root.join(&dir_part)
    };
    let rev = *revs_in(&dir, &name).last()?;
    let entry = dir.join(entry_name(&name, rev));
    let bytes = fs::metadata(&entry).ok()?.len() as usize;
    Some(Snapshot {
        display: workspace_relative(&entry, workspace_root),
        rev,
        bytes,
    })
}

/// 读回快照正文；读不到就当没有历史。
pub(crate) fn read_snapshot(snapshot: &Snapshot, workspace_root: &Path) -> Option<String> {
    fs::read_to_string(workspace_root.join(&snapshot.display)).ok()
}

/// 同目录下 `name@N` 的全部版本号（升序）。
fn revs_in(dir: &Path, name: &str) -> Vec<u32> {
    let prefix = format!("{}@", name);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut revs: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let file_name = file_name.to_str()?;
            file_name.strip_prefix(&prefix)?.parse::<u32>().ok()
        })
        .collect();
    revs.sort_unstable();
    revs
}

/// 扫描同目录下 `name@N`，取下一个序号；目录不可读时放弃快照。
fn next_rev(dir: &Path, name: &str) -> Option<u32> {
    fs::read_dir(dir).ok()?;
    let next = revs_in(dir, name).last().copied().unwrap_or(0).saturating_add(1);
    Some(next)
}

/// 只保留最新的 [`KEEP_PER_FILE`] 版；删除失败无所谓，下次再清。
fn prune(dir: &Path, name: &str, current: u32) {
    let revs = revs_in(dir, name);
    if revs.len() <= KEEP_PER_FILE as usize {
        return;
    }
    let excess = revs.len() - KEEP_PER_FILE as usize;
    for rev in revs.into_iter().filter(|rev| *rev != current).take(excess) {
        let _ = fs::remove_file(dir.join(entry_name(name, rev)));
    }
}

fn workspace_relative(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|rest| rest.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_increment_and_stay_under_temp_history() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = workspace.join("temp/pelican.py");
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");

        let first = snapshot(&target, workspace, "v1\n").expect("first snapshot");
        assert_eq!(first.display, "temp/.history/pelican.py@1");
        assert_eq!(first.rev, 1);
        assert!(workspace.join("temp/.history/pelican.py@1").exists());

        let second = snapshot(&target, workspace, "v2\n").expect("second snapshot");
        assert_eq!(second.display, "temp/.history/pelican.py@2");
        assert_eq!(
            fs::read_to_string(workspace.join("temp/.history/pelican.py@2")).expect("read"),
            "v2\n"
        );
    }

    #[test]
    fn outside_workspace_files_land_in_external_bucket() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path().join("ws");
        fs::create_dir_all(&workspace).expect("mkdir");
        let outside = root.path().join("elsewhere/sketch.py");

        let snap = snapshot(&outside, &workspace, "x = 1\n").expect("snapshot");
        assert!(snap.display.starts_with("temp/.history/_external/"));
        assert!(snap.display.contains("elsewhere"));
    }

    #[test]
    fn pruning_keeps_the_newest_versions() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = workspace.join("a.txt");
        fs::create_dir_all(workspace.join("temp/.history")).expect("mkdir");
        for rev in 1..=24u32 {
            fs::write(
                workspace.join(format!("temp/.history/a.txt@{}", rev)),
                "old\n",
            )
            .expect("seed");
        }
        let snap = snapshot(&target, workspace, "new\n").expect("snapshot");
        assert_eq!(snap.rev, 25);
        let kept = fs::read_dir(workspace.join("temp/.history"))
            .expect("read_dir")
            .flatten()
            .count();
        assert_eq!(kept, KEEP_PER_FILE as usize);
    }
    #[test]
    fn latest_points_at_the_newest_snapshot_and_reads_it_back() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = workspace.join("notes.txt");
        fs::write(&target, "v3\n").expect("seed");
        assert!(latest(&target, workspace).is_none());

        snapshot(&target, workspace, "v1\n").expect("v1");
        snapshot(&target, workspace, "v2\n").expect("v2");
        let newest = latest(&target, workspace).expect("latest");
        assert_eq!(newest.rev, 2);
        assert_eq!(newest.display, "temp/.history/notes.txt@2");
        assert_eq!(
            read_snapshot(&newest, workspace).expect("content"),
            "v2\n"
        );
    }

}
