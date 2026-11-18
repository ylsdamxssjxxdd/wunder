//! 写入前自动快照 + 游标式撤销栈。
//!
//! 存放位置固定在工作区 `temp/.history/` 下，按目标文件路径镜像，文件名追加
//! `@序号`：`temp/pelican.py` 的第 3 版是 `temp/.history/pelican.py@3`。这样
//! 快照路径本身就能被 `read_file` / `edit` 直接使用，也不会污染用户的项目目录。
//! 同目录下的 `@cursor` 记录当前停在第几态（0 = 最初的原始内容）。
//!
//! 语义是撤销栈，而不是「两版来回翻」：
//! - 第 k 版快照保存的是「第 k-1 态」的内容，游标 = k 表示文件当前是第 k 态；
//! - 写入前把旧内容压栈，游标 +1；
//! - 撤销只把游标 -1 并把该态写回文件，**不再压栈**，所以能一路退到最初；
//! - 撤销之后又写入，会先砍掉游标之上的 redo 分支，两条时间线不会混在一起。
//!
//! 约束：
//! - 只读旧内容、只写快照，失败一律返回 `None`，绝不阻断真正的写入；
//! - 与栈顶同内容不重复记账，同一份内容不会占多版；
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
/// 游标文件名后缀；`@cursor` 解析不成序号，天然不会被当成某一版。
const CURSOR_SUFFIX: &str = "@cursor";

/// 一次成功的快照：`display` 是可直接读写的相对路径。
#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub display: String,
    pub rev: u32,
    pub bytes: usize,
}

/// 撤销一步的结果：`content` 是要写回文件的内容，`cursor` 是写回后的游标。
#[derive(Debug, Clone)]
pub(crate) struct StepBack {
    pub display: String,
    pub rev: u32,
    pub cursor: u32,
    pub bytes: usize,
    pub content: String,
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

/// 覆盖/编辑前调用：把 `previous` 压栈，返回可回读的路径。
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
    let (dir, name) = locate(target, workspace_root)?;
    fs::create_dir_all(&dir).ok()?;
    let revs = revs_in(&dir, &name);
    let state = read_state(&dir, &name, &revs);
    // 栈顶已经是这份内容：再存一份只会让目录里多出同样的副本。
    if let Some(head) = revs.iter().rev().find(|rev| **rev == state.cursor) {
        let entry = dir.join(entry_name(&name, *head));
        if fs::read_to_string(&entry).ok().as_deref() == Some(previous) {
            return Some(Snapshot {
                display: workspace_relative(&entry, workspace_root),
                rev: *head,
                bytes: previous.len(),
            });
        }
    }
    // 撤销之后又写入：游标之上是已放弃的 redo 分支，先清掉再压新的一版。
    for stale in revs.iter().filter(|rev| **rev > state.cursor) {
        let _ = fs::remove_file(dir.join(entry_name(&name, *stale)));
    }
    let rev = state.cursor.saturating_add(1);
    let entry = dir.join(entry_name(&name, rev));
    atomic_write_text(&entry, previous).ok()?;
    write_cursor(&dir, &name, rev)?;
    prune(&dir, &name, rev);
    Some(Snapshot {
        display: workspace_relative(&entry, workspace_root),
        rev,
        bytes: previous.len(),
    })
}

/// 撤销一步：只读栈里退一格的快照，**不写任何新版本**。
///
/// 调用方写回 `content` 后再用 [`commit_cursor`] 落游标，写盘失败时游标不动。
pub(crate) fn step_back(target: &Path, workspace_root: &Path) -> Option<StepBack> {
    if !enabled() {
        return None;
    }
    let (dir, name) = locate(target, workspace_root)?;
    let revs = revs_in(&dir, &name);
    if revs.is_empty() {
        return None;
    }
    let state = read_state(&dir, &name, &revs);
    let from = state.cursor;
    if from == 0 || !revs.contains(&from) {
        return None;
    }
    let entry = dir.join(entry_name(&name, from));
    let content = fs::read_to_string(&entry).ok()?;
    // 旧版历史没有游标文件，无法判断真实时间线：只允许退一步，之后按最新重建。
    let cursor = if state.legacy { 0 } else { from - 1 };
    Some(StepBack {
        display: workspace_relative(&entry, workspace_root),
        rev: from,
        cursor,
        bytes: content.len(),
        content,
    })
}

/// 落游标：撤销写回成功后调用，决定下一次 undo 退到哪一格。
pub(crate) fn commit_cursor(target: &Path, workspace_root: &Path, cursor: u32) -> Option<()> {
    let (dir, name) = locate(target, workspace_root)?;
    write_cursor(&dir, &name, cursor)
}

/// 快照目录与文件名：`temp/.history/pelican.py` + `pelican.py`。
fn locate(target: &Path, workspace_root: &Path) -> Option<(PathBuf, String)> {
    let root = history_root(workspace_root);
    let mirrored = mirror_path(target, workspace_root);
    let (dir_part, name) = split_mirror(&mirrored)?;
    let dir = if dir_part.is_empty() {
        root
    } else {
        root.join(&dir_part)
    };
    Some((dir, name))
}

/// 当前栈状态：游标 + 是否来自没有游标的旧版历史。
#[derive(Debug, Clone, Copy)]
struct StackState {
    cursor: u32,
    legacy: bool,
}

fn read_state(dir: &Path, name: &str, revs: &[u32]) -> StackState {
    match read_cursor(dir, name) {
        Some(cursor) => StackState {
            cursor,
            legacy: false,
        },
        None => StackState {
            cursor: revs.last().copied().unwrap_or(0),
            legacy: true,
        },
    }
}

fn read_cursor(dir: &Path, name: &str) -> Option<u32> {
    fs::read_to_string(cursor_path(dir, name))
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()
}

fn write_cursor(dir: &Path, name: &str, cursor: u32) -> Option<()> {
    atomic_write_text(&cursor_path(dir, name), &cursor.to_string()).ok()
}

fn cursor_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{}{}", name, CURSOR_SUFFIX))
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

/// 同目录下 `name@N` 的全部版本号（升序）；`@cursor` 不是数字，自动排除。
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

    fn seed(workspace: &Path, name: &str, content: &str) -> PathBuf {
        let target = workspace.join(name);
        fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        fs::write(&target, content).expect("seed");
        target
    }

    fn read(target: &Path) -> String {
        fs::read_to_string(target).expect("read")
    }

    fn revs_of(workspace: &Path, name: &str) -> Vec<u32> {
        let (dir, file) = locate(&workspace.join(name), workspace).expect("locate");
        revs_in(&dir, &file)
    }

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
        // 旧目录没有 @cursor：游标取最新一版（24），新写入落在 25 并淘汰到 20 版。
        assert_eq!(revs_of(workspace, "a.txt").len(), KEEP_PER_FILE as usize);
    }

    #[test]
    fn undo_walks_the_whole_stack_instead_of_toggling() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = seed(workspace, "temp/seq.py", "A\n");

        // A → B → C：每次写入前压栈，栈里是 A、B，游标 2，文件是 C。
        snapshot(&target, workspace, &read(&target)).expect("snapshot A");
        fs::write(&target, "B\n").expect("write B");
        snapshot(&target, workspace, &read(&target)).expect("snapshot B");
        fs::write(&target, "C\n").expect("write C");

        // 撤销只退游标、不压栈：C → B → A，然后是「没有更多历史」。
        let first = step_back(&target, workspace).expect("undo 1");
        assert_eq!(first.content, "B\n");
        assert_eq!(first.cursor, 1);
        fs::write(&target, &first.content).expect("write back");
        commit_cursor(&target, workspace, first.cursor).expect("cursor");
        assert_eq!(read(&target), "B\n");

        let second = step_back(&target, workspace).expect("undo 2");
        assert_eq!(second.content, "A\n");
        assert_eq!(second.cursor, 0);
        fs::write(&target, &second.content).expect("write back");
        commit_cursor(&target, workspace, second.cursor).expect("cursor");
        assert_eq!(read(&target), "A\n");

        assert!(step_back(&target, workspace).is_none(), "栈底必须停住");
        // 撤销没有写新版本：目录里仍然只有 A、B 两版。
        assert_eq!(revs_of(workspace, "temp/seq.py"), vec![1, 2]);
    }

    #[test]
    fn writing_after_undo_drops_the_redo_branch() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = seed(workspace, "temp/branch.py", "A\n");

        snapshot(&target, workspace, "A\n").expect("rev1");
        fs::write(&target, "B\n").expect("write B");
        snapshot(&target, workspace, "B\n").expect("rev2");
        fs::write(&target, "C\n").expect("write C");

        // 退回到 B：C 从此是可丢弃的 redo 分支。
        let back = step_back(&target, workspace).expect("undo");
        assert_eq!(back.content, "B\n");
        fs::write(&target, &back.content).expect("restore B");
        commit_cursor(&target, workspace, back.cursor).expect("cursor");

        // 从 B 另起一版 D：序号重新落在 2，C 那条分支不会留着占位。
        let snap = snapshot(&target, workspace, "B\n").expect("snapshot");
        assert_eq!(snap.rev, 2);
        assert_eq!(revs_of(workspace, "temp/branch.py"), vec![1, 2]);
        fs::write(&target, "D\n").expect("write D");

        let undo = step_back(&target, workspace).expect("undo after branch");
        assert_eq!(undo.content, "B\n");
        assert_eq!(undo.cursor, 1);
    }

    #[test]
    fn identical_content_is_not_recorded_twice() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = seed(workspace, "temp/same.css", ".a { color: red; }\n");

        let first = snapshot(&target, workspace, &read(&target)).expect("rev1");
        assert_eq!(first.rev, 1);
        // 同内容再压一次：复用栈顶，不多占一版。
        let again = snapshot(&target, workspace, &read(&target)).expect("dedup");
        assert_eq!(again.rev, 1);
        assert_eq!(revs_of(workspace, "temp/same.css"), vec![1]);
    }

    #[test]
    fn legacy_history_without_cursor_steps_back_once() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = seed(workspace, "temp/legacy.py", "C\n");
        fs::create_dir_all(workspace.join("temp/.history")).expect("mkdir");
        fs::write(workspace.join("temp/.history/legacy.py@1"), "A\n").expect("rev1");
        fs::write(workspace.join("temp/.history/legacy.py@2"), "B\n").expect("rev2");

        // 旧目录没有 @cursor：只允许退一步，之后就按「没有更多历史」处理。
        let back = step_back(&target, workspace).expect("undo");
        assert_eq!(back.content, "B\n");
        assert_eq!(back.cursor, 0);
        fs::write(&target, &back.content).expect("write back");
        commit_cursor(&target, workspace, back.cursor).expect("cursor");
        assert!(step_back(&target, workspace).is_none());
    }
}
