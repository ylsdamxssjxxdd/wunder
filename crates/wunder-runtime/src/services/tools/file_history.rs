//! 写入前自动快照 + 游标式撤销/重做栈。
//!
//! 存放位置固定在工作区 `temp/.history/` 下，按目标文件路径镜像，文件名追加
//! `@序号`：`temp/pelican.py` 的第 3 版是 `temp/.history/pelican.py@3`。这样
//! 快照路径本身就能被 `read_file` / `edit` 直接使用，也不会污染用户的项目目录。
//! 同目录下的 `@cursor` 记录当前停在第几态（0 = 最初的原始内容）。
//!
//! 编号约定：`@k` 存的是「第 k-1 态」的内容，所以第 s 态 = `@(s+1)`。
//!
//! 语义是撤销栈，而不是「两版来回翻」：
//! - 写入前把旧内容压栈，游标 +1；
//! - 撤销只把游标 -1 并写回该态，同时把「正要离开的那一态」补进 `@(cursor+1)`
//!   作为 redo 来源，所以能一路退到最初、也能原路退回来；
//! - 重做只把游标 +1 并写回 `@(cursor+2)`；
//! - 撤销之后又写入，会先砍掉游标之上的 redo 分支，两条时间线不会混在一起。
//!
//! 回收策略：
//! - 每文件保留 [`KEEP_PER_FILE`] 版，且整份历史不超过 [`MAX_FILE_BYTES`]；
//! - 整个历史目录不超过 [`MAX_TOTAL_BYTES`]，超过则按修改时间从旧到新回收；
//! - 超过 [`MAX_AGE_SECS`] 的版本直接回收（每 [`SWEEP_INTERVAL_SECS`] 检查一次）；
//! - 超过 [`MAX_SNAPSHOT_BYTES`] 的大文件不记快照，避免把仓库撑爆；
//! - `WUNDER_FILE_HISTORY=0` 可整体关掉。

use crate::core::atomic_write::atomic_write_text;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 单个快照的内容上限；超过就跳过（只丢历史，不影响写入）。
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
/// 每个目标文件保留的版本数。
const KEEP_PER_FILE: u32 = 20;
/// 单个目标文件的历史总字节上限。
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// 整个历史目录的字节上限。
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
/// 超过这个年龄的版本直接回收。
const MAX_AGE_SECS: u64 = 14 * 24 * 3600;
/// 全局回收的最短间隔。
const SWEEP_INTERVAL_SECS: u64 = 6 * 3600;
/// 历史目录最多向下走几层（防止异常深的镜像路径拖垮回收）。
const MAX_SWEEP_DEPTH: usize = 32;
/// 工作区内的历史根目录（相对工作区根）。
const HISTORY_DIR_REL: &str = "temp/.history";
/// 工作区外文件的快照放在这个子目录下。
const EXTERNAL_DIR: &str = "_external";
/// 外部路径压缩后的最大长度。
const MAX_EXTERNAL_NAME: usize = 80;
/// 游标文件名后缀；`@cursor` 解析不成序号，天然不会被当成某一版。
const CURSOR_SUFFIX: &str = "@cursor";
/// 全局回收的时间戳标记文件。
const SWEEP_MARKER: &str = "@swept";

/// 一次成功的快照：`display` 是可直接读写的相对路径。
#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub display: String,
    pub rev: u32,
    pub bytes: usize,
}

/// 撤销/重做一步的结果：`content` 要写回文件，`cursor` 是写回后的游标。
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
    maybe_sweep(&history_root(workspace_root));
    Some(Snapshot {
        display: workspace_relative(&entry, workspace_root),
        rev,
        bytes: previous.len(),
    })
}

/// 撤销一步：写回上一态，同时把「正要离开的那一态」补成 redo 来源。
///
/// `current` 是文件此刻的内容（调用方已读过，避免再读一次盘）。只读旧快照 +
/// 补一个前进态，不新增历史态，所以连续撤销不会互相污染。
pub(crate) fn step_back(target: &Path, workspace_root: &Path, current: &str) -> Option<StepBack> {
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
    let cursor = if state.legacy {
        // 旧版历史没有游标文件，无法判断真实时间线：只允许退一步。
        0
    } else {
        // 离开的那一态（= 当前文件内容）落在 `@(cursor+1)`，redo 才有内容可写。
        keep_forward_state(&dir, &name, from + 1, current, &revs);
        from - 1
    };
    Some(StepBack {
        display: workspace_relative(&entry, workspace_root),
        rev: from,
        cursor,
        bytes: content.len(),
        content,
    })
}

/// 重做一步：写回下一态（`@(cursor+2)`），游标 +1。
pub(crate) fn step_forward(target: &Path, workspace_root: &Path) -> Option<StepBack> {
    if !enabled() {
        return None;
    }
    let (dir, name) = locate(target, workspace_root)?;
    let revs = revs_in(&dir, &name);
    if revs.is_empty() {
        return None;
    }
    let state = read_state(&dir, &name, &revs);
    let rev = state.cursor.checked_add(2)?;
    if !revs.contains(&rev) {
        return None;
    }
    let entry = dir.join(entry_name(&name, rev));
    let content = fs::read_to_string(&entry).ok()?;
    Some(StepBack {
        display: workspace_relative(&entry, workspace_root),
        rev,
        cursor: state.cursor + 1,
        bytes: content.len(),
        content,
    })
}

/// 落游标：撤销/重做写回成功后调用，决定下一次进退到哪一格。
pub(crate) fn commit_cursor(target: &Path, workspace_root: &Path, cursor: u32) -> Option<()> {
    let (dir, name) = locate(target, workspace_root)?;
    write_cursor(&dir, &name, cursor)
}

/// 把「正要离开的那一态」补进 `@slot`：缺失才写，已有同内容就不动。
fn keep_forward_state(dir: &Path, name: &str, slot: u32, current: &str, revs: &[u32]) {
    if current.len() > MAX_SNAPSHOT_BYTES {
        return;
    }
    let entry = dir.join(entry_name(name, slot));
    if revs.contains(&slot) {
        if fs::read_to_string(&entry).ok().as_deref() == Some(current) {
            return;
        }
    }
    let _ = atomic_write_text(&entry, current);
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

/// 按版本数与字节数回收：先砍最旧，再按总字节压到上限。
fn prune(dir: &Path, name: &str, current: u32) {
    let mut revs = revs_in(dir, name);
    while revs.len() > KEEP_PER_FILE as usize {
        match revs.first().copied() {
            Some(oldest) if oldest != current => {
                revs.remove(0);
                let _ = fs::remove_file(dir.join(entry_name(name, oldest)));
            }
            _ => break,
        }
    }
    let mut total: u64 = revs
        .iter()
        .filter_map(|rev| fs::metadata(dir.join(entry_name(name, *rev))).ok())
        .map(|meta| meta.len())
        .sum();
    for rev in revs {
        if total <= MAX_FILE_BYTES {
            break;
        }
        if rev == current {
            continue;
        }
        if let Ok(meta) = fs::metadata(dir.join(entry_name(name, rev))) {
            if fs::remove_file(dir.join(entry_name(name, rev))).is_ok() {
                total = total.saturating_sub(meta.len());
            }
        }
    }
}

/// 全局回收：按间隔节流，避免每次写入都扫一遍历史目录。
fn maybe_sweep(root: &Path) {
    let marker = root.join(SWEEP_MARKER);
    if let Ok(meta) = fs::metadata(&marker) {
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = SystemTime::now().duration_since(modified) {
                if age.as_secs() < SWEEP_INTERVAL_SECS {
                    return;
                }
            }
        }
    }
    if fs::create_dir_all(root).is_err() {
        return;
    }
    if atomic_write_text(&marker, "1").is_err() {
        return;
    }
    sweep(root);
}

/// 真正清扫：先删过旧的版本，再按总量从旧到新回收。
fn sweep(root: &Path) {
    let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    collect_entries(root, 0, &mut entries);
    if entries.is_empty() {
        return;
    }
    let now = SystemTime::now();
    let mut total = 0u64;
    let mut alive: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    for (path, bytes, modified) in entries {
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        if age.as_secs() > MAX_AGE_SECS {
            if fs::remove_file(&path).is_ok() {
                continue;
            }
        }
        total = total.saturating_add(bytes);
        alive.push((path, bytes, modified));
    }
    if total <= MAX_TOTAL_BYTES {
        return;
    }
    alive.sort_by(|left, right| left.2.cmp(&right.2));
    for (path, bytes, _) in alive {
        if total <= MAX_TOTAL_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(bytes);
        }
    }
}

fn collect_entries(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64, SystemTime)>) {
    if depth > MAX_SWEEP_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_entries(&path, depth + 1, out);
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // 游标与标记文件不是历史内容，不参与回收。
        if name.ends_with(CURSOR_SUFFIX) || name == SWEEP_MARKER {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((path, meta.len(), modified));
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

    /// 写一次文件：先压栈再落盘，和真实调用方的顺序一致。
    fn commit(target: &Path, workspace: &Path, before: &str, after: &str) {
        snapshot(target, workspace, before).expect("snapshot");
        fs::write(target, after).expect("write");
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
        // 没有 @cursor 的旧目录：游标取最新一版（24），新写入落在 25 并淘汰到 20 版。
        let snap = snapshot(&target, workspace, "new\n").expect("snapshot");
        assert_eq!(snap.rev, 25);
        assert_eq!(revs_of(workspace, "a.txt").len(), KEEP_PER_FILE as usize);
    }

    #[test]
    fn undo_walks_the_whole_stack_and_redo_walks_it_back() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = seed(workspace, "temp/seq.py", "A\n");

        // A → B → C：栈里是 A、B，游标 2，文件是 C。
        commit(&target, workspace, "A\n", "B\n");
        commit(&target, workspace, "B\n", "C\n");

        // 撤销：C → B → A，不新增历史态。
        let first = step_back(&target, workspace, "C\n").expect("undo 1");
        assert_eq!(first.content, "B\n");
        assert_eq!(first.cursor, 1);
        fs::write(&target, &first.content).expect("write back");
        commit_cursor(&target, workspace, first.cursor).expect("cursor");
        assert_eq!(read(&target), "B\n");

        let second = step_back(&target, workspace, "B\n").expect("undo 2");
        assert_eq!(second.content, "A\n");
        assert_eq!(second.cursor, 0);
        fs::write(&target, &second.content).expect("write back");
        commit_cursor(&target, workspace, second.cursor).expect("cursor");
        assert_eq!(read(&target), "A\n");
        assert!(step_back(&target, workspace, "A\n").is_none(), "栈底必须停住");

        // 重做原路返回：A → B → C，同样不新增历史态。
        let back = step_forward(&target, workspace).expect("redo 1");
        assert_eq!(back.content, "B\n");
        assert_eq!(back.cursor, 1);
        fs::write(&target, &back.content).expect("write back");
        commit_cursor(&target, workspace, back.cursor).expect("cursor");
        assert_eq!(read(&target), "B\n");

        let back = step_forward(&target, workspace).expect("redo 2");
        assert_eq!(back.content, "C\n");
        assert_eq!(back.cursor, 2);
        fs::write(&target, &back.content).expect("write back");
        commit_cursor(&target, workspace, back.cursor).expect("cursor");
        assert_eq!(read(&target), "C\n");
        assert!(step_forward(&target, workspace).is_none(), "栈顶必须停住");
    }

    #[test]
    fn writing_after_undo_drops_the_redo_branch() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let target = seed(workspace, "temp/branch.py", "A\n");

        commit(&target, workspace, "A\n", "B\n");
        commit(&target, workspace, "B\n", "C\n");

        // 退回到 B：C 从此是可丢弃的 redo 分支。
        let back = step_back(&target, workspace, "C\n").expect("undo");
        assert_eq!(back.content, "B\n");
        fs::write(&target, &back.content).expect("restore B");
        commit_cursor(&target, workspace, back.cursor).expect("cursor");

        // 从 B 另起一版 D：redo 分支被砍掉，重做无处可去。
        commit(&target, workspace, "B\n", "D\n");
        assert!(step_forward(&target, workspace).is_none(), "分支应已丢弃");
        assert_eq!(revs_of(workspace, "temp/branch.py"), vec![1, 2]);
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
        let back = step_back(&target, workspace, "C\n").expect("undo");
        assert_eq!(back.content, "B\n");
        assert_eq!(back.cursor, 0);
        fs::write(&target, &back.content).expect("write back");
        commit_cursor(&target, workspace, back.cursor).expect("cursor");
        assert!(step_back(&target, workspace, "B\n").is_none());
    }

    #[test]
    fn sweep_drops_versions_older_than_the_retention_window() {
        let root = tempfile::tempdir().expect("tempdir");
        let workspace = root.path();
        let history = history_root(workspace);
        fs::create_dir_all(&history).expect("mkdir");
        let old = history.join("stale.py@1");
        fs::write(&old, "x\n").expect("seed");
        // 把修改时间推到保留窗口之外。
        let stamp = SystemTime::now() - Duration::from_secs(MAX_AGE_SECS + 60);
        set_mtime(&old, stamp);

        let fresh = history.join("stale.py@2");
        fs::write(&fresh, "y\n").expect("seed");

        sweep(&history);
        assert!(!old.exists(), "过期版本应被回收");
        assert!(fresh.exists(), "新版本必须留着");
    }

    fn set_mtime(path: &Path, at: SystemTime) {
        let file = fs::File::options()
            .write(true)
            .open(path)
            .expect("open for mtime");
        file.set_modified(at).expect("set mtime");
    }
}
