//! 写入前自动快照 + 游标式撤销/重做栈。
//!
//! 存放位置固定在工作区 `temp/.history/` 下，按目标文件路径镜像，文件名形如
//! `@序号-短sha`：`temp/pelican.py` 的第 3 版是 `temp/.history/pelican.py@3-1a2b3c4d`。
//! 短 sha 是内容 sha256 的前 8 位，读快照时先核对：内容与文件名不符就当损坏丢弃，
//! 绝不把被污染的内容「恢复」进用户文件。快照路径本身仍可被 `read_file` / `edit`
//! 直接使用，也不会污染用户的项目目录。
//! 同目录下的 `@cursor` 记的是「下一次撤销要用的那个快照」的工作区相对路径（如
//! `temp/.history/pelican.py@3-1a2b3c4d`），不是下标：删掉/压实某一版时下标会
//! 漂移，路径是稳定身份，而且与 undo/redo 回执给的字符串完全一致；`0` 表示已经
//! 退回最早一态。旧目录里只写文件名的形态仍然能读。
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
//! - 整个历史目录不超过 [`MAX_TOTAL_BYTES`]（默认 32 MiB）字节、也不超过
//!   [`MAX_TOTAL_FILES`]（默认 1024）个文件，超出就按修改时间从旧到新回收
//!   （游标指向的那一版除外）；两个上限可用 `WUNDER_FILE_HISTORY_MAX_MB` /
//!   `WUNDER_FILE_HISTORY_MAX_FILES` 调；
//! - 回收闸门看账本（`@wrote` 记着上次实测的总量 + 之后写了多少）：用量不明、
//!   或账本一越过预算，当次写入就立刻实测回收；此外累计新写入超过
//!   [`SWEEP_BYTES_TRIGGER`]、距上次回收超过 [`SWEEP_INTERVAL_SECS`] 也会扫一次；
//! - 超过 [`MAX_AGE_SECS`] 的版本直接回收；
//! - 超过 [`MAX_SNAPSHOT_BYTES`] 的大文件不记快照，避免把仓库撑爆；
//! - `WUNDER_FILE_HISTORY=0` 可整体关掉。

use crate::core::atomic_write::atomic_write_text;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

mod sweep;

use sweep::maybe_sweep;
#[cfg(test)]
use sweep::{collect_entries, read_usage, sweep, sweep_with_limits, write_usage, Usage};

/// 单个快照的内容上限；超过就跳过（只丢历史，不影响写入）。
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
/// 每个目标文件保留的版本数。
const KEEP_PER_FILE: u32 = 20;
/// 单个目标文件的历史总字节上限。
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// 整个历史目录的字节上限。
///
/// 历史是「改坏了能退回一步」的保险，不是备份仓库：32 MiB 已经覆盖几十个文件的
/// 二十版内容，再占下去只是白吃本地磁盘（尤其老机器）。可用
/// `WUNDER_FILE_HISTORY_MAX_MB` 调大或调小。
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
/// 历史目录里的文件数上限（约等于「多少份快照」）。
///
/// 只按字节算不够：几十 KB 的小文件能把 inode 和目录扫描拖垮，字节数却还很低。
const MAX_TOTAL_FILES: usize = 1024;
/// 超过这个年龄的版本直接回收。
const MAX_AGE_SECS: u64 = 14 * 24 * 3600;
/// 全局回收的最短间隔。
const SWEEP_INTERVAL_SECS: u64 = 6 * 3600;
/// 历史目录最多向下走几层（防止异常深的镜像路径拖垮回收）。
const MAX_SWEEP_DEPTH: usize = 32;
/// 工作区内的历史根目录（相对工作区根）。
const HISTORY_DIR_REL: &str = crate::services::workspace_temp::FILE_HISTORY;
/// 工作区外文件的快照放在这个子目录下。
const EXTERNAL_DIR: &str = "_external";
/// 外部路径压缩后的最大长度。
const MAX_EXTERNAL_NAME: usize = 80;
/// 游标文件名后缀；`@cursor` 解析不成序号，天然不会被当成某一版。
const CURSOR_SUFFIX: &str = "@cursor";
/// 游标落盘文本里的「已到最早」。
const CURSOR_AT_OLDEST: &str = "0";
/// 全局回收的时间戳标记文件。
const SWEEP_MARKER: &str = "@swept";
/// 用量记录：`<总字节> <文件数> <自上次实测以来写入的字节>`。
///
/// 每次都扫全目录太贵，于是记一份账：写入时累加，超预算就直接实测并回收。
/// 记账只会偏大（`prune` 删掉的那些没来得及扣），偏大只会多扫一次，不会漏。
const USAGE_MARKER: &str = "@wrote";
/// 累计写入超过这个量就直接回收一次，不等时间间隔到期。
///
/// 时间间隔只能保证「最坏 6 小时扫一次」，两次扫描之间可能灌进远超上限的历史；
/// 用写入量兜住这一头：总量最坏也就 `MAX_TOTAL_BYTES + SWEEP_BYTES_TRIGGER`。
const SWEEP_BYTES_TRIGGER: u64 = 8 * 1024 * 1024;
/// 快照文件名里带的内容短 sha 长度（sha256 前 8 位十六进制）。
const SHORT_HASH_LEN: usize = 8;

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
    /// 游标落盘文本：下一次撤销用的快照文件名，或 `0`（已在最早）。
    pub cursor: String,
    /// 回执展示：下一次能撤销到的快照路径；已在最早时为 `None`。
    pub cursor_display: Option<String>,
    pub bytes: usize,
    pub content: String,
}

/// 退不动的原因：调用方据此给出不同错误码，而不是一律说「没有快照」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StepBlock {
    /// 这个文件从未留下过历史（没改写、快照被关掉、目录不可用）。
    NoHistory,
    /// 有历史，但游标已经在最早一态。
    AtOldest { revisions: u32 },
    /// 有历史，但游标已经在最新一态。
    AtNewest,
    /// 目标版本的快照内容与文件名里的短 sha 不符：按损坏处理并已删除。
    Corrupt { display: String },
}

/// 读快照失败：缺失，或内容与文件名里的短 sha 不符（已被删除）。
#[derive(Debug, PartialEq, Eq)]
enum SnapshotError {
    Missing,
    Corrupt(PathBuf),
}

/// 一次成功的快照读取：路径要回填给回执，内容要写回文件。
#[derive(Debug)]
struct SnapshotRead {
    path: PathBuf,
    content: String,
}

/// 读快照失败时统一收口：损坏的一版要连带压实编号与游标，否则下标与文件编号
/// 错位，之后的 undo 会读到错位内容，或者干脆宣布「已到最早」跳过更早的历史。
fn discard_block(
    dir: &Path,
    name: &str,
    rev: u32,
    err: SnapshotError,
    workspace_root: &Path,
) -> StepBlock {
    match err {
        SnapshotError::Missing => StepBlock::NoHistory,
        SnapshotError::Corrupt(path) => {
            let display = workspace_relative(&path, workspace_root);
            drop_revision(dir, name, rev, workspace_root);
            StepBlock::Corrupt { display }
        }
    }
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
pub(crate) fn snapshot(target: &Path, workspace_root: &Path, previous: &str) -> Option<Snapshot> {
    if !enabled() || previous.len() > MAX_SNAPSHOT_BYTES {
        return None;
    }
    let (dir, name) = locate(target, workspace_root)?;
    fs::create_dir_all(&dir).ok()?;
    let (revs, state) = live_revs(&dir, &name);
    let base = state.cursor_rev(&revs);
    // 栈顶已经是这份内容：再存一份只会让目录里多出同样的副本。
    if revs.contains(&base)
        && read_snapshot(&dir, &name, base)
            .ok()
            .map(|read| read.content)
            .as_deref()
            == Some(previous)
    {
        if let Some(entry) = existing_entry(&dir, &name, base) {
            return Some(Snapshot {
                display: workspace_relative(&entry, workspace_root),
                rev: base,
                bytes: previous.len(),
            });
        }
    }
    // 撤销之后又写入：游标之上是已放弃的 redo 分支，先清掉再压新的一版。
    for stale in revs.iter().filter(|rev| **rev > base) {
        remove_rev(&dir, &name, *stale);
    }
    let rev = base.saturating_add(1);
    let entry = dir.join(entry_name(&name, rev, previous));
    atomic_write_text(&entry, previous).ok()?;
    // 游标记的是文件名，不是下标。
    write_cursor_at(&dir, &name, Some(rev), workspace_root)?;
    prune(&dir, &name, rev);
    maybe_sweep(
        &history_root(workspace_root),
        workspace_root,
        previous.len() as u64,
    );
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
pub(crate) fn step_back(
    target: &Path,
    workspace_root: &Path,
    current: &str,
) -> Result<StepBack, StepBlock> {
    if !enabled() {
        return Err(StepBlock::NoHistory);
    }
    let (dir, name) = locate(target, workspace_root).ok_or(StepBlock::NoHistory)?;
    let (revs, state) = live_revs(&dir, &name);
    if revs.is_empty() {
        return Err(StepBlock::NoHistory);
    }
    // 中间某一版可能已经被删（损坏回收、手工清理）：退到游标之下最近还在的一版，
    // 不能直接宣布「已到最早」，那会把更早的历史永久跳过。
    let cursor_rev = state.cursor_rev(&revs);
    let from = if cursor_rev == 0 {
        None
    } else {
        nearest_at_or_below(&revs, cursor_rev)
    };
    let Some(from) = from else {
        return Err(StepBlock::AtOldest {
            revisions: revs.len() as u32,
        });
    };
    let read = read_snapshot(&dir, &name, from)
        .map_err(|err| discard_block(&dir, &name, from, err, workspace_root))?;
    let next = if state.legacy() {
        // 旧版历史没有游标文件，无法判断真实时间线：只允许退一步。
        Cursor::AtOldest
    } else {
        // 离开的那一态（= 当前文件内容）落在游标之上，redo 才有内容可写。
        keep_forward_state(&dir, &name, from + 1, current, &revs);
        cursor_at(&dir, &name, from - 1, workspace_root)
    };
    Ok(StepBack {
        display: workspace_relative(&read.path, workspace_root),
        rev: from,
        cursor: next.storage(),
        cursor_display: next.display(&dir, workspace_root),
        bytes: read.content.len(),
        content: read.content,
    })
}

/// 重做一步：写回下一态（`@(cursor+2)`），游标 +1。
pub(crate) fn step_forward(target: &Path, workspace_root: &Path) -> Result<StepBack, StepBlock> {
    if !enabled() {
        return Err(StepBlock::NoHistory);
    }
    let (dir, name) = locate(target, workspace_root).ok_or(StepBlock::NoHistory)?;
    let (revs, state) = live_revs(&dir, &name);
    if revs.is_empty() {
        return Err(StepBlock::NoHistory);
    }
    let rev = state
        .cursor_rev(&revs)
        .checked_add(2)
        .filter(|rev| revs.contains(rev))
        .ok_or(StepBlock::AtNewest)?;
    let read = read_snapshot(&dir, &name, rev)
        .map_err(|err| discard_block(&dir, &name, rev, err, workspace_root))?;
    // 重做之后，下一次撤销要用的是刚离开的那一格 `@(rev-1)`。
    let next = cursor_at(&dir, &name, rev - 1, workspace_root);
    Ok(StepBack {
        display: workspace_relative(&read.path, workspace_root),
        rev,
        cursor: next.storage(),
        cursor_display: next.display(&dir, workspace_root),
        bytes: read.content.len(),
        content: read.content,
    })
}

/// 落游标：撤销/重做写回成功后调用，决定下一次进退到哪一格。
pub(crate) fn commit_cursor(target: &Path, workspace_root: &Path, cursor: &str) -> Option<()> {
    let (dir, name) = locate(target, workspace_root)?;
    write_cursor(&dir, &name, cursor)
}

/// 把「正要离开的那一态」补进 `@slot`：缺失才写，已有同内容就不动。
fn keep_forward_state(dir: &Path, name: &str, slot: u32, current: &str, revs: &[u32]) {
    if current.len() > MAX_SNAPSHOT_BYTES {
        return;
    }
    if revs.contains(&slot)
        && read_snapshot(dir, name, slot)
            .ok()
            .map(|read| read.content)
            .as_deref()
            == Some(current)
    {
        return;
    }
    // 同一版本号只会有一份内容：先清掉旧文件，再按新内容写新名字。
    remove_rev(dir, name, slot);
    let entry = dir.join(entry_name(name, slot, current));
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

/// 游标：直接记「下一次撤销要用的那个快照文件名」，而不是下标。
///
/// 下标会随删除/压实漂移，还容易被误读成「第几个版本」；文件名是稳定身份，
/// 目录被人动过也能一眼看出指向哪一格。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Cursor {
    /// 已经在最早一态，没有更早的快照。
    AtOldest,
    /// 旧版历史留下的下标（读旧目录时兼容）。
    Index(u32),
    /// 快照文件的工作区相对路径，例如 `temp/.history/v6_ok.css@2-0fd445b0`。
    ///
    /// 回执里给出的也是同一个字符串：游标文件与回执不再有两种写法。
    File(String),
}

impl Cursor {
    /// 游标对应的编号；`AtOldest` 为 0。
    fn rev(&self) -> Option<u32> {
        match self {
            Cursor::AtOldest => Some(0),
            Cursor::Index(index) => Some(*index),
            Cursor::File(file) => rev_in_file_name(file),
        }
    }

    /// 落盘文本：`0` / 下标 / 文件名。
    fn storage(&self) -> String {
        match self {
            Cursor::AtOldest => CURSOR_AT_OLDEST.to_string(),
            Cursor::Index(index) => index.to_string(),
            Cursor::File(file) => file.clone(),
        }
    }

    /// 回执展示：下一次能撤销到的快照路径；已在最早时为 `None`。
    fn display(&self, dir: &Path, workspace_root: &Path) -> Option<String> {
        match self {
            Cursor::AtOldest => None,
            // 旧版下标没有对应文件，回执里原样给出数字，别假装是路径。
            Cursor::Index(index) => Some(index.to_string()),
            // 老游标只存了文件名，回执还要补成工作区相对路径；新游标原样返回。
            Cursor::File(file) if file.contains('/') => Some(file.clone()),
            Cursor::File(file) => Some(workspace_relative(&dir.join(file), workspace_root)),
        }
    }
}

/// 当前栈状态：`cursor == None` 表示旧版目录（没有 `@cursor`）。
struct StackState {
    cursor: Option<Cursor>,
}

impl StackState {
    fn legacy(&self) -> bool {
        self.cursor.is_none()
    }

    /// 游标对应的编号；旧目录不知道真实时间线，按栈顶处理。
    fn cursor_rev(&self, revs: &[u32]) -> u32 {
        match &self.cursor {
            None => revs.last().copied().unwrap_or(0),
            Some(cursor) => cursor.rev().unwrap_or(0),
        }
    }
}

/// 读栈：版本号 + 游标状态。
///
/// 游标之上的版本是 redo 分支，长度本身没有上限（退到最早时整条前进链都在），
/// 所以这里不按游标裁剪；伪造的版本号由下一次写入「砍 redo 分支」时清掉。
fn live_revs(dir: &Path, name: &str) -> (Vec<u32>, StackState) {
    let revs = revs_in(dir, name);
    let state = StackState {
        cursor: read_cursor(dir, name),
    };
    (revs, state)
}

/// 编号不超过 `cursor` 的最近一版：中间有洞时用它兜底。
fn nearest_at_or_below(revs: &[u32], cursor: u32) -> Option<u32> {
    revs.iter().copied().filter(|rev| *rev <= cursor).max()
}

/// 游标指向「下一次撤销要用的快照」；`rev == 0` 表示没有更早的了。
///
/// 文件不在（被删/被改名）时退回下标形式，让后续查找继续往下探，而不是
/// 直接宣布到底。
fn cursor_at(dir: &Path, name: &str, rev: u32, workspace_root: &Path) -> Cursor {
    if rev == 0 {
        return Cursor::AtOldest;
    }
    match entry_file_name(dir, name, rev) {
        Some(file) => Cursor::File(workspace_relative(&dir.join(file), workspace_root)),
        None => Cursor::Index(rev),
    }
}

/// 游标文本 → 末段文件名；兼容「工作区相对路径」与旧版「只写文件名」两种形态。
fn cursor_basename(text: &str) -> Option<&str> {
    text.rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
}

/// 快照文件名 → 编号：`v6_ok.css@2-0fd445b0` → 2。
fn rev_in_file_name(file: &str) -> Option<u32> {
    let (_, tail) = file.rsplit_once('@')?;
    tail.split('-').next()?.parse::<u32>().ok()
}

fn entry_file_name(dir: &Path, name: &str, rev: u32) -> Option<String> {
    existing_entry(dir, name, rev)?
        .file_name()?
        .to_str()
        .map(str::to_string)
}

fn read_cursor(dir: &Path, name: &str) -> Option<Cursor> {
    let raw = fs::read_to_string(cursor_path(dir, name)).ok()?;
    Some(parse_cursor(raw.trim()))
}

/// `0` → 已在最早；纯数字 → 旧版下标；其余 → 快照文件名。
fn parse_cursor(raw: &str) -> Cursor {
    if raw.is_empty() || raw == CURSOR_AT_OLDEST {
        return Cursor::AtOldest;
    }
    match raw.parse::<u32>() {
        Ok(index) => Cursor::Index(index),
        Err(_) => Cursor::File(raw.to_string()),
    }
}

fn write_cursor(dir: &Path, name: &str, text: &str) -> Option<()> {
    atomic_write_text(&cursor_path(dir, name), text).ok()
}

/// 把游标写成某一版的文件名；`None` 表示已到最早。
fn write_cursor_at(dir: &Path, name: &str, rev: Option<u32>, workspace_root: &Path) -> Option<()> {
    match rev {
        None => write_cursor(dir, name, CURSOR_AT_OLDEST),
        Some(rev) => {
            let file = entry_file_name(dir, name, rev)?;
            // 与回执同一种写法：工作区相对路径。
            let text = workspace_relative(&dir.join(file), workspace_root);
            write_cursor(dir, name, &text)
        }
    }
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

fn entry_name(name: &str, rev: u32, content: &str) -> String {
    format!("{}@{}-{}", name, rev, short_hash(content))
}

/// 旧版快照没有 sha 后缀，只作兼容读取，不再新写。
fn legacy_entry_name(name: &str, rev: u32) -> String {
    format!("{}@{}", name, rev)
}

/// 内容短 sha：sha256 的前 8 位十六进制。
fn short_hash(content: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    hex::encode(digest).chars().take(SHORT_HASH_LEN).collect()
}

/// 某一版实际存在的文件：优先带 sha 的新命名，其次旧版命名。
fn existing_entry(dir: &Path, name: &str, rev: u32) -> Option<PathBuf> {
    let prefix = format!("{}@{}-", name, rev);
    let legacy = legacy_entry_name(name, rev);
    let mut fallback = None;
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.starts_with(&prefix) {
            return Some(entry.path());
        }
        if file_name == legacy {
            fallback = Some(entry.path());
        }
    }
    fallback
}

/// 读一版快照并核对内容：文件名里的短 sha 与内容不符就当损坏，删掉并报错。
fn read_snapshot(dir: &Path, name: &str, rev: u32) -> Result<SnapshotRead, SnapshotError> {
    let path = existing_entry(dir, name, rev).ok_or(SnapshotError::Missing)?;
    let content = fs::read_to_string(&path).map_err(|_| SnapshotError::Missing)?;
    let expected = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|file_name| file_name.strip_prefix(&format!("{}@{}-", name, rev)))
        .map(str::to_string);
    if let Some(expected) = expected {
        if short_hash(&content) != expected {
            // 读的时候不删：删一格必须连带压实编号和游标，交给调用方统一处理。
            return Err(SnapshotError::Corrupt(path));
        }
    }
    Ok(SnapshotRead { path, content })
}

/// 删掉某一版并把后面的编号整体前移一格，游标同步减一。
///
/// 快照按编号落盘、游标按下标记位：中间少一格就会错位，所以删除必须压实，
/// 否则 `@k` 与「第 k-1 态」的约定就断了。
fn drop_revision(dir: &Path, name: &str, rev: u32, workspace_root: &Path) {
    let revs = revs_in(dir, name);
    remove_rev(dir, name, rev);
    for later in revs.iter().copied().filter(|candidate| *candidate > rev) {
        let Some(path) = existing_entry(dir, name, later) else {
            continue;
        };
        let Some(target) = renumbered_path(&path, name, later, later - 1) else {
            continue;
        };
        let _ = fs::rename(&path, &target);
    }
    rebase_cursor(dir, name, workspace_root);
}

/// 删除/压实之后修正游标。
///
/// 新版游标记的是快照路径：文件被改名或删掉就失效，此时退到「编号不超过它的
/// 最近一版」，实在没有就落 `0`。旧版下标则按剩下还在的版本重新对齐。
fn rebase_cursor(dir: &Path, name: &str, workspace_root: &Path) {
    let Some(cursor) = read_cursor(dir, name) else {
        return;
    };
    let revs = revs_in(dir, name);
    match cursor {
        Cursor::AtOldest => {}
        Cursor::Index(index) => match nearest_at_or_below(&revs, index) {
            Some(rev) if rev == index => {}
            Some(rev) => {
                let _ = write_cursor(dir, name, &rev.to_string());
            }
            None => {
                let _ = write_cursor(dir, name, CURSOR_AT_OLDEST);
            }
        },
        Cursor::File(file) => {
            let base = cursor_basename(&file).unwrap_or_default();
            if dir.join(base).exists() {
                return;
            }
            let rev = rev_in_file_name(&file).unwrap_or(0);
            let next = nearest_at_or_below(&revs, rev);
            let _ = write_cursor_at(dir, name, next, workspace_root);
        }
    }
}

/// `name@7-1a2b3c4d` → `name@6-1a2b3c4d`：只改编号，短 sha 与其他后缀原样保留。
fn renumbered_path(path: &Path, name: &str, from: u32, to: u32) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    let rest = file_name.strip_prefix(&format!("{}@{}", name, from))?;
    Some(path.with_file_name(format!("{}@{}{}", name, to, rest)))
}

/// 删掉某一版的全部文件（新旧两种命名都算）。
fn remove_rev(dir: &Path, name: &str, rev: u32) {
    let prefix = format!("{}@{}-", name, rev);
    let legacy = legacy_entry_name(name, rev);
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.starts_with(&prefix) || file_name == legacy {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn rev_bytes(dir: &Path, name: &str, rev: u32) -> Option<u64> {
    let path = existing_entry(dir, name, rev)?;
    fs::metadata(path).ok().map(|meta| meta.len())
}

/// 同目录下 `name@N` / `name@N-<sha>` 的全部版本号（升序）。
/// `@cursor` 是文件名后缀而不是 `name@`，天然不会被当成某一版。
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
            let rest = file_name.strip_prefix(&prefix)?;
            rest.split('-').next()?.parse::<u32>().ok()
        })
        .collect();
    revs.sort_unstable();
    revs.dedup();
    revs
}

/// 按版本数与字节数回收：先砍最旧，再按总字节压到上限。
fn prune(dir: &Path, name: &str, current: u32) {
    let mut revs = revs_in(dir, name);
    while revs.len() > KEEP_PER_FILE as usize {
        match revs.first().copied() {
            Some(oldest) if oldest != current => {
                revs.remove(0);
                remove_rev(dir, name, oldest);
            }
            _ => break,
        }
    }
    let mut total: u64 = revs
        .iter()
        .filter_map(|rev| rev_bytes(dir, name, *rev))
        .sum();
    for rev in revs {
        if total <= MAX_FILE_BYTES {
            break;
        }
        if rev == current {
            continue;
        }
        let bytes = rev_bytes(dir, name, rev).unwrap_or(0);
        remove_rev(dir, name, rev);
        total = total.saturating_sub(bytes);
    }
}

fn workspace_relative(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(|rest| rest.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
#[path = "../file_history_tests.rs"]
mod tests;
