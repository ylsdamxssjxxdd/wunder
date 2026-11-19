//! 历史目录的预算与回收：用量账本、清扫触发、按大小/份数/过期回收、编号压实。
//!
//! 与栈逻辑（`mod.rs`）分开，是为了让「保多少、砍哪些」能独立演进：这里是唯一
//! 删除快照文件的地方，删除时必须让游标与编号保持一致（见 `compact_removed`）。

use super::*;

/// 目录用量：总字节、文件数，以及自上次实测以来写入的字节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Usage {
    pub(super) bytes: u64,
    pub(super) files: usize,
    pub(super) written: u64,
}

/// 全局回收的触发条件：用量不明、超预算、写入够多，或时间到了。
///
/// 只看时间会留一个空档——两次扫描之间可以灌进任意量历史，所以还要看账本：
/// 账本记着「上次实测的总量 + 之后又写了多少」，一旦越过预算就立刻实测回收。
/// 完全没有账本（第一次、或目录是别人手造/拷进来的）就直接实测一次。
pub(super) fn maybe_sweep(root: &Path, workspace_root: &Path, added: u64) {
    if fs::create_dir_all(root).is_err() {
        return;
    }
    let cap_bytes = total_bytes_limit();
    let cap_files = file_count_limit();
    let usage = bump_usage(root, added);
    let due = match &usage {
        None => true,
        Some(usage) => {
            usage.bytes > cap_bytes
                || usage.files > cap_files
                || usage.written > SWEEP_BYTES_TRIGGER
                || marker_age_secs(root) >= SWEEP_INTERVAL_SECS
        }
    };
    if !due {
        return;
    }
    // 先落时间戳再扫：扫的过程中再有写入进来，也不会引发第二轮全目录扫描。
    if atomic_write_text(&root.join(SWEEP_MARKER), "1").is_err() {
        return;
    }
    let measured = sweep_with_limits(root, workspace_root, cap_bytes, cap_files);
    write_usage(root, measured);
}

/// 距上次回收过了多久；没有标记文件时按「很久以前」处理。
fn marker_age_secs(root: &Path) -> u64 {
    fs::metadata(root.join(SWEEP_MARKER))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age.as_secs())
        .unwrap_or(u64::MAX)
}

/// 把这次写入累加进账本；账本缺失或读不懂就返回 `None`（调用方据此实测一次）。
fn bump_usage(root: &Path, added: u64) -> Option<Usage> {
    let mut usage = read_usage(root)?;
    usage.bytes = usage.bytes.saturating_add(added);
    usage.files = usage.files.saturating_add(1);
    usage.written = usage.written.saturating_add(added);
    write_usage(root, usage);
    Some(usage)
}

/// 读账本：`<总字节> <文件数> [自上次实测以来写入]`；旧格式只有字节数，当作未知。
pub(super) fn read_usage(root: &Path) -> Option<Usage> {
    let text = fs::read_to_string(root.join(USAGE_MARKER)).ok()?;
    let mut parts = text.split_whitespace();
    let bytes = parts.next()?.parse::<u64>().ok()?;
    let files = parts.next()?.parse::<usize>().ok()?;
    let written = parts
        .next()
        .and_then(|token| token.parse::<u64>().ok())
        .unwrap_or(0);
    Some(Usage {
        bytes,
        files,
        written,
    })
}

pub(super) fn write_usage(root: &Path, usage: Usage) {
    let text = format!("{} {} {}", usage.bytes, usage.files, usage.written);
    let _ = atomic_write_text(&root.join(USAGE_MARKER), &text);
}

/// 真正清扫：过老的版本先删，再按总量与文件数从旧到新回收。
pub(super) fn sweep(root: &Path, workspace_root: &Path) {
    let cap_bytes = total_bytes_limit();
    let cap_files = file_count_limit();
    let measured = sweep_with_limits(root, workspace_root, cap_bytes, cap_files);
    write_usage(root, measured);
}

/// 总量上限（默认 [`MAX_TOTAL_BYTES`]，可用 `WUNDER_FILE_HISTORY_MAX_MB` 调）。
fn total_bytes_limit() -> u64 {
    std::env::var("WUNDER_FILE_HISTORY_MAX_MB")
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        .filter(|mb| *mb > 0)
        .map(|mb| mb.saturating_mul(1024 * 1024))
        .unwrap_or(MAX_TOTAL_BYTES)
}

/// 文件数上限（默认 [`MAX_TOTAL_FILES`]，可用 `WUNDER_FILE_HISTORY_MAX_FILES` 调）。
fn file_count_limit() -> usize {
    std::env::var("WUNDER_FILE_HISTORY_MAX_FILES")
        .ok()
        .and_then(|text| text.trim().parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(MAX_TOTAL_FILES)
}

/// 带显式上限的清扫：按修改时间从旧到新删，直到总量与文件数都装得下。
///
/// 游标指向的那一版受保护：它是「下一次撤销」的唯一入口，删掉就等于让用户
/// 按一次撤销悄悄跳过一步。其余一律最旧优先。
pub(super) fn sweep_with_limits(
    root: &Path,
    workspace_root: &Path,
    max_bytes: u64,
    max_files: usize,
) -> Usage {
    let mut entries: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    let mut protected: HashSet<PathBuf> = HashSet::new();
    collect_entries(root, 0, &mut entries, &mut protected);
    if entries.is_empty() {
        return Usage::default();
    }
    let now = SystemTime::now();
    let mut total = 0u64;
    let mut alive: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    let mut removed: Vec<PathBuf> = Vec::new();
    for (path, bytes, modified) in entries {
        // 时钟回拨时 duration_since 会报错：按「不算过期」处理，别拿它当回收依据。
        let expired = now
            .duration_since(modified)
            .unwrap_or(Duration::ZERO)
            .as_secs()
            > MAX_AGE_SECS;
        if expired && !protected.contains(&path) && fs::remove_file(&path).is_ok() {
            removed.push(path);
            continue;
        }
        total = total.saturating_add(bytes);
        alive.push((path, bytes, modified));
    }
    alive.sort_by(|left, right| left.2.cmp(&right.2));
    let mut alive_count = alive.len();
    for (path, bytes, _) in alive {
        if total <= max_bytes && alive_count <= max_files {
            break;
        }
        if protected.contains(&path) {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(bytes);
            alive_count = alive_count.saturating_sub(1);
            removed.push(path);
        }
    }
    // 回收掉的是「某一版」而不是随便一个文件：不压实编号，历史中间就会留洞。
    compact_removed(&removed, workspace_root);
    Usage {
        bytes: total,
        files: alive_count,
        written: 0,
    }
}

/// 回收后压实受影响的栈。按版本号从大到小处理：`drop_revision` 只会把更大的
/// 编号往前挪，所以大号先删，小号的编号不会被打乱。
fn compact_removed(paths: &[PathBuf], workspace_root: &Path) {
    if paths.is_empty() {
        return;
    }
    let mut stacks: HashMap<(PathBuf, String), Vec<u32>> = HashMap::new();
    for path in paths {
        let Some((dir, name, rev)) = split_entry_path(path) else {
            continue;
        };
        stacks.entry((dir, name)).or_default().push(rev);
    }
    for ((dir, name), mut revs) in stacks {
        revs.sort_unstable_by(|left, right| right.cmp(left));
        for rev in revs {
            drop_revision(&dir, &name, rev, workspace_root);
        }
    }
}

/// `…/name@7-1a2b3c4d` 或旧版 `…/name@7` →（目录, name, 7）。
fn split_entry_path(path: &Path) -> Option<(PathBuf, String, u32)> {
    let file_name = path.file_name()?.to_str()?;
    let (stem, rest) = file_name.rsplit_once('@')?;
    let rev = rest.split('-').next()?.parse::<u32>().ok()?;
    Some((path.parent()?.to_path_buf(), stem.to_string(), rev))
}

/// 收集历史目录下的快照文件，同时把「游标指向的那一版」记进 `protected`。
pub(super) fn collect_entries(
    dir: &Path,
    depth: usize,
    out: &mut Vec<(PathBuf, u64, SystemTime)>,
    protected: &mut HashSet<PathBuf>,
) {
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
            collect_entries(&path, depth + 1, out, protected);
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        // 游标与标记文件不是历史内容，不参与回收；游标指向的那一版要护住。
        if name.ends_with(CURSOR_SUFFIX) {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Some(base) = cursor_basename(text.trim()) {
                    protected.insert(dir.join(base));
                }
            }
            continue;
        }
        if name == SWEEP_MARKER || name == USAGE_MARKER {
            continue;
        }
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((path, meta.len(), modified));
    }
}
