//! 行级 diff 辅助：把（旧文本，新文本）转成与补丁家族**逐字一致**的块/行结构。
//!
//! 存在的理由：`apply_patch` 早已返回结构化 diff，但 `写入文件` 与字面替换类
//! 编辑工具不返回任何 diff，渲染侧无法画出差异卡片。本模块让三者共用同一份
//! 键名与取值词汇（见 [`insert_diff_fields`]），一个渲染器即可消费全部写文件工具。
//!
//! 契约（与 `apply_patch_tool` 的 `data`/`files[]`/`diff_blocks[]`/`lines[]` 对齐）：
//! - `kind` 只有 `meta`/`add`/`delete`；`text` 是**裸行内容**，不带 `+`/`-`/空格前缀。
//! - `old_line`/`new_line` 为 1-based，对侧没有行时为 `null`。
//! - 计数（`added_lines`/`deleted_lines`/`hunks`）永远反映真实变更；被块上限或
//!   行预算裁掉的行只累加进 `diff_lines_omitted`，前端不得从预览行数推算变更量。
//!
//! 算法不依赖外部 diff：先裁公共首尾行，再用「两侧都只出现一次」的行作锚点
//! 递归切分，确实找不到锚点才退回粗粒度区域。行文本借用原文切片不复制；扫描量
//! 有总预算兜底，病态内容不会退化成平方级。

use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// 单次工具调用最多输出的块数；超出的块整块折进 `diff_lines_omitted`。
/// 回显是「确认改在哪里」，不是「重放整个文件」：模型的下一步只需要块头、
/// 变更行号和少量上下文，其余靠 `changed_lines` + 磁盘文件核对。
pub(super) const MAX_DIFF_BLOCKS: usize = 4;
/// 可见行上限（与 `apply_patch_tool/preview.rs` 的每文件预算一致；本模块每次
/// 调用只处理一个文件，因此每文件即每次调用）。
const MAX_DIFF_LINES: usize = 24;
/// 行正文上限。
const MAX_DIFF_TEXT_BYTES: usize = 8 * 1024;
/// 允许参与 diff 的最大文本字节数；任一侧到界即放弃 diff（只给计数）。
pub(super) const MAX_DIFF_SOURCE_BYTES: usize = 512 * 1024;

/// `diff_unavailable` 的短原因串取值集合。
pub(super) const DIFF_UNAVAILABLE_OVERSIZE: &str = "oversize";
pub(super) const DIFF_UNAVAILABLE_BINARY: &str = "binary";
pub(super) const DIFF_UNAVAILABLE_UNREADABLE: &str = "unreadable";

/// 结构化块省略原因：单行太长，正文放不下。
pub(super) const OMITTED_LINE_TOO_LONG: &str = "line_too_long";
/// 结构化块省略原因：行数/块数预算用完。
pub(super) const OMITTED_BUDGET: &str = "budget";

#[derive(Debug, Clone)]
struct DiffLine {
    kind: &'static str,
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: String,
}

#[derive(Debug, Clone)]
struct DiffBlock {
    header: String,
    start_line_before: usize,
    end_line_before: usize,
    start_line_after: usize,
    end_line_after: usize,
    lines: Vec<DiffLine>,
}

/// 一次工具调用的 diff 结果：有界块 + 准确计数。
#[derive(Debug, Clone, Default)]
pub(super) struct LineDiff {
    blocks: Vec<DiffBlock>,
    added_lines: usize,
    deleted_lines: usize,
    omitted_lines: usize,
    /// 检测到的变更区域数，对应补丁家族的 `hunks`；不受块上限影响。
    hunks: usize,
    visible_lines: usize,
    text_bytes: usize,
    /// 因为「单行太长」被丢掉的正文行；与块预算/行预算区分开，回执要说清原因。
    long_lines: usize,
}

/// 一个变更区域；一侧行数为 0 表示纯插入/纯删除。
#[derive(Debug, Clone, Copy)]
struct Region {
    old_start: usize,
    old_len: usize,
    new_start: usize,
    new_len: usize,
}

impl LineDiff {
    /// 用调用方已知的准确区域数覆盖，供命中采样被上限截断时使用。
    pub(super) fn with_hunks(mut self, hunks: usize) -> Self {
        self.hunks = hunks;
        self
    }

    /// 有没有「看得见的 diff 被省掉了」；返回省掉的原因。
    ///
    /// 调用方必须据此显式告诉用户「这里没有完整的 diff」，否则只有 `---`/`+++`
    /// 头的 `unified_diff` 会被读成「没有改动」。
    pub(super) fn omission(&self) -> Option<&'static str> {
        if self.long_lines > 0 {
            return Some(OMITTED_LINE_TOO_LONG);
        }
        if self.omitted_lines > 0 {
            return Some(OMITTED_BUDGET);
        }
        None
    }

    /// 行正文预算：超预算的整行省略，只累加 `omitted_lines`。
    fn push_row(&mut self, rows: &mut Vec<DiffLine>, kind: &'static str, line: usize, text: &str) {
        // 单行超预算：不是「截断了」，而是这一行根本放不下，回执要单独说明。
        if text.len() > MAX_DIFF_TEXT_BYTES.saturating_sub(self.text_bytes) {
            self.long_lines += 1;
            self.omitted_lines += 1;
            return;
        }
        if self.visible_lines >= MAX_DIFF_LINES {
            self.omitted_lines += 1;
            return;
        }
        self.visible_lines += 1;
        self.text_bytes += text.len();
        rows.push(DiffLine {
            kind,
            old_line: (kind == "delete").then_some(line),
            new_line: (kind == "add").then_some(line),
            text: text.to_string(),
        });
    }

    /// 计数始终准确；块上限命中后整块折进 `omitted_lines`。
    fn push_region(&mut self, old: &[&str], new: &[&str], region: Region) {
        let region = clamp_region(old, new, region);
        if region.old_len == 0 && region.new_len == 0 {
            return;
        }
        self.hunks += 1;
        self.added_lines += region.new_len;
        self.deleted_lines += region.old_len;
        if self.blocks.len() >= MAX_DIFF_BLOCKS {
            self.omitted_lines += region.old_len + region.new_len;
            return;
        }
        let mut rows = Vec::with_capacity(region.old_len + region.new_len);
        for (offset, text) in old[region.old_start..region.old_start + region.old_len]
            .iter()
            .enumerate()
        {
            self.push_row(&mut rows, "delete", region.old_start + offset + 1, text);
        }
        for (offset, text) in new[region.new_start..region.new_start + region.new_len]
            .iter()
            .enumerate()
        {
            self.push_row(&mut rows, "add", region.new_start + offset + 1, text);
        }
        if rows.is_empty() {
            return;
        }
        self.blocks.push(DiffBlock {
            header: format!(
                "@@ -{},{} +{},{} @@",
                region.old_start + 1,
                region.old_len,
                region.new_start + 1,
                region.new_len
            ),
            start_line_before: region.old_start + 1,
            end_line_before: if region.old_len == 0 {
                region.old_start
            } else {
                region.old_start + region.old_len
            },
            start_line_after: region.new_start + 1,
            end_line_after: if region.new_len == 0 {
                region.new_start
            } else {
                region.new_start + region.new_len
            },
            lines: rows,
        });
    }
}

/// 行索引：行文本与行首字节偏移一次遍历同时得到，文本借用原文不复制。
struct LineIndex<'a> {
    content: &'a str,
    texts: Vec<&'a str>,
    starts: Vec<usize>,
}

impl LineIndex<'_> {
    /// 字节偏移所属的 0-based 行号；落在内容末尾之外时按虚拟尾行处理。
    fn line_of(&self, byte: usize) -> usize {
        let total = self.texts.len();
        if byte >= self.content.len() {
            if self.content.is_empty() || self.content.ends_with('\n') {
                return total;
            }
            return total.saturating_sub(1);
        }
        self.starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1)
    }
}

/// 与 `apply_patch_tool::split_lines` 相同的行语义（只去掉一个尾随空行），
/// 但不为每行分配 `String`：CRLF 仅裁掉行尾 `\r`，lone-CR 视作行内容。
fn build_line_index(content: &str) -> LineIndex<'_> {
    let mut index = LineIndex {
        content,
        texts: Vec::new(),
        starts: Vec::new(),
    };
    let mut cursor = 0usize;
    loop {
        let next = content[cursor..].find('\n').map(|offset| cursor + offset);
        let raw = match next {
            Some(end) => &content[cursor..end],
            None => &content[cursor..],
        };
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        if next.is_none() && text.is_empty() {
            break;
        }
        index.texts.push(text);
        index.starts.push(cursor);
        match next {
            Some(end) => cursor = end + 1,
            None => break,
        }
    }
    index
}

/// 区间一律夹到真实行数内：输入再离谱也只可能少报，不会越界或 panic。
fn clamp_region(old: &[&str], new: &[&str], region: Region) -> Region {
    let old_start = region.old_start.min(old.len());
    let new_start = region.new_start.min(new.len());
    Region {
        old_start,
        old_len: region.old_len.min(old.len() - old_start),
        new_start,
        new_len: region.new_len.min(new.len() - new_start),
    }
}

/// 新建文件：整份内容一个块，每行 `kind:"add"`，块头与行号区间和补丁家族的
/// `new file` 完全一致。热路径：不读盘、不建行索引，只流式扫一遍新内容，
/// 额外内存只有实际渲染的那几行。
pub(super) fn diff_added_file(content: &str) -> LineDiff {
    let mut diff = LineDiff::default();
    diff.hunks = 1;
    let mut rows = Vec::new();
    // 与 `split_lines` 同规则：尾随换行不额外产生空行。
    let last_segment = content
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    let mut total = 0usize;
    for (index, raw) in content.split('\n').enumerate() {
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        if index == last_segment && text.is_empty() {
            break;
        }
        total = index + 1;
        diff.push_row(&mut rows, "add", total, text);
    }
    diff.added_lines = total;
    diff.blocks.push(DiffBlock {
        header: "new file".to_string(),
        start_line_before: 0,
        end_line_before: 0,
        start_line_after: 1,
        end_line_after: total,
        lines: rows,
    });
    diff
}

/// 覆盖写：按真实对齐切成变更区域，再交给有界块渲染。
pub(super) fn diff_replaced(before: &str, after: &str) -> LineDiff {
    let old = build_line_index(before);
    let new = build_line_index(after);
    let mut diff = LineDiff::default();
    for region in changed_regions(&old.texts, &new.texts) {
        diff.push_region(&old.texts, &new.texts, region);
    }
    diff
}

/// 完整的变更区域（无块数/行数预算）。
///
/// `LineDiff` 用它生成有界块供前端差异卡片使用；`unified_diff` 在块被预算裁过
/// 时用它重算完整文本。两处必须是同一份算法，否则模型看到的 diff 会和
/// `diff_blocks` 对不上。
fn changed_regions(old: &[&str], new: &[&str]) -> Vec<Region> {
    split_regions(old, new, 0, old.len(), 0, new.len())
}

/// 把两个行区间切成升序、互不重叠的变更区域。
///
/// 直接在中间段做「先删后增」会把「改一行 + 后面插入两行」报成删掉/新增几百行，
/// 所以先裁公共首尾行，再用两侧都只出现一次的行当锚点递归切分；只有确实没有
/// 锚点时才退回粗粒度（等长按位置切连续不等段，不等长整段）。`pending` 用显式
/// 栈而不是递归，避免超长文件把调用栈打穿。
fn split_regions(
    old: &[&str],
    new: &[&str],
    old_from: usize,
    old_to: usize,
    new_from: usize,
    new_to: usize,
) -> Vec<Region> {
    let mut regions = Vec::new();
    // 扫描预算：每轮按剩余行数扣减，超预算的区间退回粗粒度。没有它，极端内容
    // （例如整份倒序）会退化成平方级扫描。
    let mut budget = (old_to - old_from + new_to - new_from).saturating_mul(4) + 4096;
    let mut pending = vec![(old_from, old_to, new_from, new_to)];
    while let Some((mut old_start, mut old_end, mut new_start, mut new_end)) = pending.pop() {
        while old_start < old_end && new_start < new_end && old[old_start] == new[new_start] {
            old_start += 1;
            new_start += 1;
        }
        while old_start < old_end && new_start < new_end && old[old_end - 1] == new[new_end - 1] {
            old_end -= 1;
            new_end -= 1;
        }
        if old_start == old_end && new_start == new_end {
            continue;
        }
        let old_len = old_end - old_start;
        let new_len = new_end - new_start;
        if old_len == 0 || new_len == 0 {
            regions.push(Region {
                old_start,
                old_len,
                new_start,
                new_len,
            });
            continue;
        }
        let anchors = if old_len + new_len <= budget {
            budget -= old_len + new_len;
            unique_anchors(old, new, old_start, old_end, new_start, new_end)
        } else {
            Vec::new()
        };
        if anchors.is_empty() {
            if old_len == new_len {
                push_positional_runs(old, new, old_start, new_start, old_len, &mut regions);
            } else {
                regions.push(Region {
                    old_start,
                    old_len,
                    new_start,
                    new_len,
                });
            }
            continue;
        }
        // 锚点本身是未变更行，两侧在锚点处切开；倒序入栈保证输出按旧侧行号升序。
        let mut ranges = Vec::with_capacity(anchors.len() + 1);
        let (mut prev_old, mut prev_new) = (old_start, new_start);
        for (anchor_old, anchor_new) in anchors {
            ranges.push((prev_old, anchor_old, prev_new, anchor_new));
            prev_old = anchor_old + 1;
            prev_new = anchor_new + 1;
        }
        ranges.push((prev_old, old_end, prev_new, new_end));
        for range in ranges.into_iter().rev() {
            if range.0 < range.1 || range.2 < range.3 {
                pending.push(range);
            }
        }
    }
    regions
}

/// 等长的中间段没有锚点时，按对齐位置把连续不等的位置合成区域。
fn push_positional_runs(
    old: &[&str],
    new: &[&str],
    old_start: usize,
    new_start: usize,
    len: usize,
    regions: &mut Vec<Region>,
) {
    let mut run: Option<Region> = None;
    for offset in 0..len {
        if old[old_start + offset] == new[new_start + offset] {
            if let Some(region) = run.take() {
                regions.push(region);
            }
            continue;
        }
        match run.as_mut() {
            Some(region) => {
                region.old_len += 1;
                region.new_len += 1;
            }
            None => {
                run = Some(Region {
                    old_start: old_start + offset,
                    old_len: 1,
                    new_start: new_start + offset,
                    new_len: 1,
                })
            }
        }
    }
    if let Some(region) = run {
        regions.push(region);
    }
}

#[derive(Clone, Copy, Default)]
struct AnchorStat {
    old_count: u32,
    new_count: u32,
    new_index: usize,
}

/// 两侧都恰好出现一次的行作为锚点，按旧侧顺序取新侧下标的最长递增子序列：
/// 只有相对顺序一致的锚点才能把两侧安全地切开。
fn unique_anchors(
    old: &[&str],
    new: &[&str],
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
) -> Vec<(usize, usize)> {
    let mut stats: HashMap<&str, AnchorStat> = HashMap::new();
    for line in &old[old_start..old_end] {
        stats.entry(*line).or_default().old_count += 1;
    }
    for (offset, line) in new[new_start..new_end].iter().enumerate() {
        if let Some(stat) = stats.get_mut(*line) {
            stat.new_count += 1;
            stat.new_index = new_start + offset;
        }
    }
    let mut pairs = Vec::new();
    for (offset, line) in old[old_start..old_end].iter().enumerate() {
        let Some(stat) = stats.get(*line) else {
            continue;
        };
        if stat.old_count == 1 && stat.new_count == 1 {
            pairs.push((old_start + offset, stat.new_index));
        }
    }
    longest_increasing_pairs(pairs)
}

/// 对 (旧侧下标, 新侧下标) 取新侧下标的最长递增子序列，返回其中的锚点对。
fn longest_increasing_pairs(pairs: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut tails: Vec<usize> = Vec::new();
    let mut parent: Vec<Option<usize>> = vec![None; pairs.len()];
    for (index, &(_, new_index)) in pairs.iter().enumerate() {
        let slot = tails.partition_point(|&tail| pairs[tail].1 < new_index);
        if slot > 0 {
            parent[index] = Some(tails[slot - 1]);
        }
        if slot == tails.len() {
            tails.push(index);
        } else {
            tails[slot] = index;
        }
    }
    let mut chain = Vec::with_capacity(tails.len());
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        chain.push(pairs[index]);
        cursor = parent[index];
    }
    chain.reverse();
    chain
}

/// 字面替换：`offsets` 是 `before` 中每个被替换区间的字节起点（升序、互不重叠），
/// 一个区间一个块；新侧行号由累计位移推出，不做二次全文扫描。
/// 两侧任一超出 diff 源上限时不给块，只回 `oversize` 原因。
pub(super) fn diff_literal_replaces(
    before: &str,
    after: &str,
    offsets: &[usize],
    needle_len: usize,
    replacement: &str,
) -> (LineDiff, Option<&'static str>) {
    if diff_source_too_large(before, after) {
        return (LineDiff::default(), Some(DIFF_UNAVAILABLE_OVERSIZE));
    }
    let old = build_line_index(before);
    let new = build_line_index(after);
    let mut diff = LineDiff::default();
    if needle_len == 0 {
        return (diff, None);
    }
    let mut delta: isize = 0;
    for &offset in offsets {
        let Some(end) = offset.checked_add(needle_len) else {
            break;
        };
        if offset >= before.len() || end > before.len() {
            break;
        }
        let old_start = old.line_of(offset);
        let old_len = old.line_of(end - 1).saturating_sub(old_start) + 1;
        let new_offset = (offset as isize + delta).max(0) as usize;
        let new_start = new.line_of(new_offset);
        let new_len = if replacement.is_empty() {
            0
        } else {
            new.line_of(new_offset + replacement.len() - 1)
                .saturating_sub(new_start)
                + 1
        };
        // 命中区间的行范围通常比真实变更大：一次替换可能只改其中一行、其余是
        // 插入，所以同样按锚点切分，计数只反映真实改动。
        let regions = split_regions(
            &old.texts,
            &new.texts,
            old_start,
            old_start + old_len,
            new_start,
            new_start + new_len,
        );
        for region in regions {
            diff.push_region(&old.texts, &new.texts, region);
        }
        delta += replacement.len() as isize - needle_len as isize;
    }
    (diff, None)
}

/// 两侧任一超出 diff 源上限就放弃 diff（调用方据此给出 `oversize`）。
pub(super) fn diff_source_too_large(before: &str, after: &str) -> bool {
    before.len() > MAX_DIFF_SOURCE_BYTES || after.len() > MAX_DIFF_SOURCE_BYTES
}

fn block_json(block: &DiffBlock) -> Value {
    json!({
        "header": block.header,
        "start_line_before": block.start_line_before,
        "end_line_before": block.end_line_before,
        "start_line_after": block.start_line_after,
        "end_line_after": block.end_line_after,
        "lines": block.lines.iter().map(|line| json!({
            "kind": line.kind,
            "old_line": line.old_line,
            "new_line": line.new_line,
            "text": line.text,
        })).collect::<Vec<_>>(),
    })
}

fn file_entry(action: &str, path: &str, diff: &LineDiff, unknown: bool) -> Value {
    // `unknown` 表示有界性守卫放弃了 diff：此时计数是「没算」，不是「0 行」。
    let count = |value: usize| if unknown { Value::Null } else { json!(value) };
    json!({
        "action": action,
        "path": path,
        "to_path": Value::Null,
        "hunks": count(diff.hunks),
        "added_lines": count(diff.added_lines),
        "deleted_lines": count(diff.deleted_lines),
        "diff_lines_omitted": count(diff.omitted_lines),
        "diff_blocks": diff.blocks.iter().map(block_json).collect::<Vec<_>>(),
    })
}

/// 写入 `data` 的补丁家族键（单文件变更）。默认 `LineDiff` 表示「只有计数、
/// 没有块」，用于有界性守卫放弃 diff 的场景。
///
/// `unavailable` 有值时计数一律写 `null`：文件确实改过了，报 `0` 会被读成
/// 「改了 0 行」，比不给数字更容易误导。
pub(super) fn insert_diff_fields(
    map: &mut Map<String, Value>,
    action: &str,
    path: &str,
    diff: &LineDiff,
    unavailable: Option<&'static str>,
) {
    let unknown = unavailable.is_some();
    let count = |value: usize| if unknown { Value::Null } else { json!(value) };
    map.insert("added_lines".to_string(), count(diff.added_lines));
    map.insert("deleted_lines".to_string(), count(diff.deleted_lines));
    map.insert("changed_files".to_string(), json!(1usize));
    map.insert("added".to_string(), json!(usize::from(action == "add")));
    map.insert(
        "updated".to_string(),
        json!(usize::from(action == "update")),
    );
    map.insert(
        "deleted".to_string(),
        json!(usize::from(action == "delete")),
    );
    map.insert("moved".to_string(), json!(0usize));
    map.insert("hunks_applied".to_string(), count(diff.hunks));
    map.insert("diff_lines_omitted".to_string(), count(diff.omitted_lines));
    map.insert(
        "files".to_string(),
        json!([file_entry(action, path, diff, unknown)]),
    );
}

/// 每个变更块前后各带几行上下文。
const UNIFIED_CONTEXT_LINES: usize = 2;

/// 模型面 unified diff 的行数预算。
///
/// 回显的用途是「确认改在哪里、改了多少」，不是把整段正文重放给模型：写类
/// 调用参数里已经有模型自己给出的正文，磁盘上有权威结果，`changed_lines` 提供
/// 定位。没有上限时一次 40 行重写就会带回 80+ 行 diff，并跟着之后每一次请求
/// 重复计费（实测这一步能把写类结果从 ~4 KB 压到 ~0.6 KB）。
const MAX_UNIFIED_DIFF_LINES: usize = MAX_DIFF_LINES;

/// 模型面 unified diff 的字节预算；与行数预算共同生效，放不下的块折成
/// 「块头 + 采样行 + 省略行数」。
pub(super) const MAX_UNIFIED_DIFF_BYTES: usize = MAX_DIFF_TEXT_BYTES;

/// 折叠一个超预算的块时，删除侧/新增侧各保留几行采样，让模型仍能看出
/// 改动的内容长什么样，而不是只剩一个块头。
const UNIFIED_FOLD_SAMPLE_LINES: usize = 3;

/// 给末尾的省略说明预留的空间，保证它不会被预算本身挤掉。
const UNIFIED_DIFF_MARKER_RESERVE: usize = 256;

/// 渲染结果：文本 + 被省略的规模。调用方据此写 `unified_diff_truncated`。
pub(super) struct UnifiedDiff {
    pub(super) text: String,
    pub(super) omitted_hunks: usize,
    pub(super) omitted_lines: usize,
}

impl UnifiedDiff {
    /// 有正文被省略时返回 true。
    pub(super) fn truncated(&self) -> bool {
        self.omitted_hunks > 0 || self.omitted_lines > 0
    }
}

/// 写类回执的模型面形态：**有界** unified diff + 是否发生了折叠。
///
/// 预算由 [`MAX_UNIFIED_DIFF_LINES`] 与 [`MAX_UNIFIED_DIFF_BYTES`] 共同约束：
/// 小改动（最常见的单点改几行）仍然整段可见；大改动只保留块头、删除/新增两侧
/// 各 [`UNIFIED_FOLD_SAMPLE_LINES`] 行采样与准确的省略计数。写类调用参数里已经
/// 有模型自己给出的正文，磁盘与 `changed_lines` 提供定位，正文没必要在之后每一次
/// 请求里重复携带。
pub(super) fn render_write_diff(
    path: &str,
    before: &str,
    after: &str,
    diff: &LineDiff,
) -> (String, bool) {
    let rendered = unified_diff(path, before, after, diff);
    let truncated = rendered.truncated();
    (rendered.text, truncated)
}

/// 每个变更块的起始行号（新文本侧，1-based）。
///
/// 回显被预算裁掉正文时，模型仍需要「改在哪几行」来定位下一次编辑；
/// 这份列表只与块头有关，比正文便宜两个数量级，所以每次都给。
pub(super) fn changed_lines(diff: &LineDiff) -> Vec<usize> {
    diff.blocks
        .iter()
        .map(|block| block.start_line_after.max(1))
        .collect()
}

/// 把（旧文本，新文本）渲染成 unified diff 文本：`---`/`+++`/`@@` + 上下文 + `-`/`+`。
///
/// 写类工具（写入文件、文本编辑）的回执靠这段文本确认「自己刚写进去的到底是
/// 什么」，所以它不受观察层的通用头尾截断，由本模块自己的行数/字节预算收口：
/// 小改动整段可见，大改动折成「块头 + 两侧采样 + 省略行数」，计数始终准确，
/// 模型仍能看出改了哪里、改了多少。
///
/// 源文本超过 `MAX_DIFF_SOURCE_BYTES` 时调用方会给出 `diff_unavailable`，
/// 但写类回执仍要尽力回显：这里按完整文本重算区域，再受安全阀约束。
pub(super) fn unified_diff(path: &str, before: &str, after: &str, diff: &LineDiff) -> UnifiedDiff {
    let old_index = build_line_index(before);
    let new_index = build_line_index(after);
    let old = &old_index.texts;
    let new = &new_index.texts;
    // 只有「结构化块完整」时才直接复用；块被行数/单行/块数预算裁过时必须按
    // 完整文本重算区域，否则模型面 diff 会缺掉被裁掉的部分（甚至只剩文件头）。
    let regions: Vec<Region> = if diff.omission().is_none() && !diff_source_too_large(before, after)
    {
        diff.blocks.iter().map(block_region).collect()
    } else {
        changed_regions(old, new)
    };

    let content_budget = MAX_UNIFIED_DIFF_BYTES.saturating_sub(UNIFIED_DIFF_MARKER_RESERVE);
    let mut used = 0usize;
    let mut out: Vec<String> = Vec::with_capacity(16);
    for line in [format!("--- a/{path}"), format!("+++ b/{path}")] {
        used += line_bytes(&line);
        out.push(line);
    }
    // 已输出到的旧文本行（0-based，不含）。上下文统一从旧文本取：旧实现用 new
    // 行号去限制 old 上下文，插入/删除后两侧行号错位，边界行会在两个块里各打印
    // 一次（同一个 `+TOKEN 2` 既出现在后置上下文又出现在下一块前置上下文）。
    let mut emitted_old_end = 0usize;
    let mut omitted_hunks = 0usize;
    let mut omitted_lines = 0usize;
    let mut exhausted = false;

    for (index, region) in regions.iter().enumerate() {
        if exhausted {
            omitted_hunks += 1;
            omitted_lines += region.old_len + region.new_len;
            continue;
        }
        let old_start = region.old_start;
        let old_len = region.old_len;
        let new_start = region.new_start;
        let new_len = region.new_len;
        // 下一块的删除区起点：后置上下文不能把下一块要改的行当成上下文先打印。
        let next_old_start = regions
            .get(index + 1)
            .map(|next| next.old_start)
            .unwrap_or(old.len());

        // 前置上下文：最多两行，且不与上一块已经输出过的行重叠。
        let context_from = old_start
            .saturating_sub(UNIFIED_CONTEXT_LINES)
            .max(emitted_old_end);
        let pre = old.get(context_from..old_start).unwrap_or(&[]);
        // 后置上下文：接在删除区之后，不越过下一块的删除区起点。
        let post_from = old_start + old_len;
        let post_to = (post_from + UNIFIED_CONTEXT_LINES)
            .min(next_old_start)
            .min(old.len());
        let post = old.get(post_from..post_to).unwrap_or(&[]);
        let removed = old.get(old_start..old_start + old_len).unwrap_or(&[]);
        let added = new.get(new_start..new_start + new_len).unwrap_or(&[]);

        let mut header = String::from("@@ -");
        header.push_str(&(old_start + 1).to_string());
        header.push(',');
        header.push_str(&old_len.to_string());
        header.push_str(" +");
        header.push_str(&(new_start + 1).to_string());
        header.push(',');
        header.push_str(&new_len.to_string());
        header.push_str(" @@");

        let needed = line_bytes(&header)
            + lines_bytes(pre)
            + lines_bytes(removed)
            + lines_bytes(added)
            + lines_bytes(post);
        let needed_lines = 1 + pre.len() + removed.len() + added.len() + post.len();
        // 末尾的省略说明要占一行，所以整块放行的条件是「行数 + 1」仍在预算内。
        if used + needed <= content_budget
            && out.len() + needed_lines + 1 <= MAX_UNIFIED_DIFF_LINES
        {
            used += needed;
            out.push(header);
            push_lines(&mut out, " ", pre);
            push_lines(&mut out, "-", removed);
            push_lines(&mut out, "+", added);
            push_lines(&mut out, " ", post);
            emitted_old_end = post_to.max(post_from);
            continue;
        }

        // 整块放不下：保留块头与两侧采样行，其余折成一行省略说明。模型仍能
        // 看出「改了哪里、改了什么样子」，而正文由磁盘与 `changed_lines` 兜底。
        let removed_sample = &removed[..removed.len().min(UNIFIED_FOLD_SAMPLE_LINES)];
        let added_sample = &added[..added.len().min(UNIFIED_FOLD_SAMPLE_LINES)];
        let folded_tail = format!(
            "... {} removed / {} added line(s) omitted (diff budget; file on disk is authoritative)",
            old_len.saturating_sub(removed_sample.len()),
            new_len.saturating_sub(added_sample.len())
        );
        let folded_bytes = line_bytes(&header)
            + lines_bytes(removed_sample)
            + lines_bytes(added_sample)
            + line_bytes(&folded_tail);
        let folded_lines = 2 + removed_sample.len() + added_sample.len();
        if used + folded_bytes <= content_budget
            && out.len() + folded_lines + 1 <= MAX_UNIFIED_DIFF_LINES
        {
            used += folded_bytes;
            out.push(header);
            push_lines(&mut out, "-", removed_sample);
            push_lines(&mut out, "+", added_sample);
            out.push(folded_tail);
            omitted_lines += old_len.saturating_sub(removed_sample.len())
                + new_len.saturating_sub(added_sample.len());
            emitted_old_end = post_to.max(post_from);
            continue;
        }
        // 连块头加采样都放不下：后面的块只记账，不再输出。
        exhausted = true;
        omitted_hunks += 1;
        omitted_lines += old_len + new_len;
    }

    if omitted_hunks > 0 || omitted_lines > 0 {
        let mut parts: Vec<String> = Vec::new();
        if omitted_hunks > 0 {
            parts.push(format!("{omitted_hunks} hunk(s)"));
        }
        if omitted_lines > 0 {
            parts.push(format!("{omitted_lines} line(s)"));
        }
        out.push(format!("... {} omitted (diff budget)", parts.join(" / ")));
    }

    UnifiedDiff {
        text: out.join("\n"),
        omitted_hunks,
        omitted_lines,
    }
}

/// 一行 diff 的字节数：前缀 1 字节 + 正文 + 换行。
fn line_bytes(line: &str) -> usize {
    line.len() + 2
}

fn lines_bytes(lines: &[&str]) -> usize {
    lines.iter().map(|text| text.len() + 2).sum()
}

fn push_lines(out: &mut Vec<String>, prefix: &str, lines: &[&str]) {
    for text in lines {
        out.push(prefix.to_string() + text);
    }
}

/// 从有界块推回 0-based 区域，语义与 `changed_regions` 一致。
fn block_region(block: &DiffBlock) -> Region {
    Region {
        old_start: block.start_line_before.saturating_sub(1),
        old_len: if block.start_line_before > 0 && block.end_line_before >= block.start_line_before
        {
            block.end_line_before - block.start_line_before + 1
        } else {
            0
        },
        new_start: block.start_line_after.saturating_sub(1),
        new_len: if block.end_line_after >= block.start_line_after {
            block.end_line_after - block.start_line_after + 1
        } else {
            0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_huge_line_folds_to_counts_only() {
        // 200 KB 单行：结构化块放不下（省略为 line_too_long），模型面文本连
        // 采样行都放不下，于是只保留准确的省略计数——正文在磁盘上，模型自己
        // 刚给过这份内容，没必要在回执里再背一遍。
        let before = "x".repeat(200 * 1024);
        let after = "y".repeat(200 * 1024);
        let diff = diff_replaced(&before, &after);
        assert_eq!(diff.added_lines, 1);
        assert_eq!(diff.deleted_lines, 1);
        assert_eq!(diff.hunks, 1);
        assert!(diff.blocks.is_empty(), "放不下的行不能硬塞半行");
        assert_eq!(diff.omission(), Some(OMITTED_LINE_TOO_LONG));
        assert_eq!(diff.omitted_lines, 2);

        let rendered = unified_diff("longline.txt", &before, &after, &diff);
        assert!(rendered.truncated());
        assert!(
            rendered.text.len() <= MAX_UNIFIED_DIFF_BYTES,
            "{}",
            rendered.text.len()
        );
        assert_eq!(rendered.omitted_lines, 2);
        assert!(
            rendered.text.contains("2 line(s) omitted (diff budget)"),
            "{}",
            rendered.text
        );
        assert!(!rendered.text.contains("xxxxxxxx"));
    }

    #[test]
    fn unified_diff_shows_context_and_change_lines() {
        let before = "a\nb\nc\nd\ne\n";
        let after = "a\nb\nC\nd\ne\n";
        let diff = diff_replaced(before, after);
        let text = unified_diff("x.txt", before, after, &diff).text;
        assert!(text.starts_with("--- a/x.txt\n+++ b/x.txt\n"), "{}", text);
        assert!(text.contains("@@ -3,1 +3,1 @@"), "{}", text);
        assert!(text.contains("\n a\n b\n"), "{}", text);
        assert!(text.contains("\n-c\n+C\n"), "{}", text);
    }

    #[test]
    fn unified_diff_bounds_a_full_rewrite_and_keeps_counts() {
        // 200 行全改：结构化块按块/行预算折叠，模型面文本只留两侧采样，
        // 其余折成准确的省略计数。
        let mut before = String::new();
        let mut after = String::new();
        for index in 0..200 {
            before.push_str(&format!("line {}\n", index));
            after.push_str(&format!("line {}!\n", index));
        }
        let diff = diff_replaced(&before, &after);
        assert!(diff.omission().is_some(), "结构化块应当命中预算");
        assert_eq!(diff.added_lines, 200);
        assert_eq!(diff.deleted_lines, 200);
        let rendered = unified_diff("x.txt", &before, &after, &diff);
        let text = &rendered.text;
        assert!(text.contains("@@ -1,200 +1,200 @@"), "{}", text);
        let lines: Vec<&str> = text.lines().collect();
        // 删除侧/新增侧各保留 UNIFIED_FOLD_SAMPLE_LINES 行采样。
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with('-') && !line.starts_with("---"))
                .count(),
            UNIFIED_FOLD_SAMPLE_LINES,
            "{text}"
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.starts_with('+') && !line.starts_with("+++"))
                .count(),
            UNIFIED_FOLD_SAMPLE_LINES,
            "{text}"
        );
        assert!(text.contains("-line 0"), "{text}");
        assert!(text.contains("+line 0!"), "{text}");
        assert!(
            text.contains("197 removed / 197 added line(s) omitted"),
            "{text}"
        );
        assert_eq!(rendered.omitted_lines, 394);
        assert!(text.len() <= MAX_UNIFIED_DIFF_BYTES);
    }

    #[test]
    fn unified_diff_bounds_many_hunks_to_the_line_budget() {
        // 5000 行里隔行改：2500 个 hunk，行数预算只放得下前几块，其余折成
        // 「块头 + 采样 + 省略计数」，绝不按全量回显。
        let mut before = String::new();
        let mut after = String::new();
        for index in 0..5000 {
            let line = format!("key-{:04}-value\n", index);
            before.push_str(&line);
            if index % 2 == 0 {
                after.push_str(&format!("key-{:04}-VALUE\n", index));
            } else {
                after.push_str(&line);
            }
        }
        let diff = diff_replaced(&before, &after);
        assert_eq!(diff.hunks, 2500);
        let rendered = unified_diff("big.txt", &before, &after, &diff);
        assert!(rendered.truncated());
        assert!(
            rendered.text.lines().count() <= MAX_UNIFIED_DIFF_LINES + 1,
            "{}",
            rendered.text.lines().count()
        );
        assert!(rendered.text.contains("+key-0000-VALUE"));
        assert!(!rendered.text.contains("+key-4998-VALUE"));
        assert!(
            rendered.omitted_hunks + rendered.omitted_lines > 0,
            "omitted_hunks={} omitted_lines={}",
            rendered.omitted_hunks,
            rendered.omitted_lines
        );
    }

    #[test]
    fn unified_diff_folds_a_pathological_line_and_keeps_counts_exact() {
        // 3 MiB 单行属于病态输出：结构化块与模型面文本都放不下，只剩总计行，
        // 但省略计数必须准确。
        let before = "x".repeat(3 * 1024 * 1024);
        let after = "y".repeat(3 * 1024 * 1024);
        let diff = diff_replaced(&before, &after);
        let rendered = unified_diff("huge.txt", &before, &after, &diff);
        assert!(rendered.truncated());
        assert!(
            rendered.text.len() <= MAX_UNIFIED_DIFF_BYTES,
            "{}",
            rendered.text.len()
        );
        assert_eq!(rendered.omitted_lines, 2);
        assert!(
            rendered
                .text
                .contains("2 line(s) omitted (diff budget)"),
            "{}",
            rendered.text
        );
    }

    #[test]
    fn a_large_deletion_keeps_a_head_sample_and_counts() {
        // 3000 行整体替换成一行：正文不再整段重放，只给块头 + 前 3 行采样 +
        // 省略计数，`added_lines`/`deleted_lines` 仍然准确。
        let mut before = String::new();
        for index in 0..3000 {
            before.push_str(&format!("line {:05} value\n", index));
        }
        let after = "only line\n";
        let diff = diff_replaced(&before, after);
        let rendered = unified_diff("editme.txt", &before, after, &diff);
        assert!(rendered.truncated());
        assert!(rendered.text.contains("-line 00000 value"));
        assert!(!rendered.text.contains("-line 02999 value"));
        assert!(rendered.text.contains("+only line"));
        assert!(rendered.text.contains("2997 removed / 0 added"));
    }

    #[test]
    fn a_whole_file_rewrite_folds_into_counts_only() {
        // 单行 10 KiB × 300 行 ≈ 3 MiB：连采样行都超预算，只保留总计行。
        let line_before = format!("{}\n", "o".repeat(10 * 1024));
        let line_after = format!("{}\n", "n".repeat(10 * 1024));
        let before = line_before.repeat(300);
        let after = line_after.repeat(300);
        let diff = diff_replaced(&before, &after);
        let rendered = unified_diff("rewrite.txt", &before, &after, &diff);
        assert!(rendered.truncated());
        assert_eq!(rendered.omitted_hunks, 1);
        assert_eq!(rendered.omitted_lines, 600);
        assert!(rendered.text.len() <= MAX_UNIFIED_DIFF_BYTES);
        assert!(
            rendered
                .text
                .contains("... 1 hunk(s) / 600 line(s) omitted (diff budget)"),
            "{}",
            rendered.text
        );
    }

    #[test]
    fn adjacent_blocks_do_not_repeat_context_lines() {
        // 交替替换：每块前后都有边界行，同一行只能在 diff 里出现一次。
        let before = "x0\nk1\nx2\nk3\nx4\nk5\n";
        let after = "y0\nk1\ny2\nk3\ny4\nk5\n";
        let diff = diff_replaced(before, after);
        assert_eq!(diff.hunks, 3);
        let text = unified_diff("x.txt", before, after, &diff).text;
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(text.matches("@@ -").count(), 3, "{text}");
        for context in [" k1", " k3", " k5"] {
            assert_eq!(
                lines.iter().filter(|line| **line == context).count(),
                1,
                "context {context} repeated in {text}"
            );
        }
        // 头 + 3 块 *（头 + -/+ 两行 + 1 行后置上下文）
        assert_eq!(lines.len(), 2 + 3 * 4, "{text}");
        assert!(text.contains("@@ -3,1 +3,1 @@"), "{text}");
    }

    fn kinds(diff: &LineDiff) -> Vec<&'static str> {
        diff.blocks
            .iter()
            .flat_map(|block| block.lines.iter().map(|line| line.kind))
            .collect()
    }

    fn replaces(
        before: &str,
        after: &str,
        offsets: &[usize],
        needle: &str,
        replacement: &str,
    ) -> LineDiff {
        let (diff, unavailable) =
            diff_literal_replaces(before, after, offsets, needle.len(), replacement);
        assert!(unavailable.is_none(), "样本应当能出块");
        diff
    }

    fn sorted_keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn line_index_matches_patch_family_line_semantics() {
        assert_eq!(build_line_index("a\nb\n").texts, vec!["a", "b"]);
        assert_eq!(build_line_index("a\nb").texts, vec!["a", "b"]);
        assert_eq!(build_line_index("").texts, Vec::<&str>::new());
        assert_eq!(build_line_index("\n").texts, vec![""]);
        assert_eq!(build_line_index("a\r\nb\r\n").texts, vec!["a", "b"]);
    }

    #[test]
    fn added_file_is_one_all_add_block() {
        let diff = diff_added_file("one\ntwo\nthree\n");
        assert_eq!(diff.added_lines, 3);
        assert_eq!(diff.deleted_lines, 0);
        assert_eq!(diff.hunks, 1);
        assert_eq!(diff.blocks.len(), 1);
        assert_eq!(diff.blocks[0].header, "new file");
        assert_eq!(diff.blocks[0].start_line_before, 0);
        assert_eq!(diff.blocks[0].end_line_before, 0);
        assert_eq!(diff.blocks[0].start_line_after, 1);
        assert_eq!(diff.blocks[0].end_line_after, 3);
        assert_eq!(kinds(&diff), vec!["add", "add", "add"]);
        assert_eq!(diff.blocks[0].lines[1].text, "two");
        assert_eq!(diff.blocks[0].lines[1].old_line, None);
        assert_eq!(diff.blocks[0].lines[1].new_line, Some(2));
    }

    #[test]
    fn added_file_text_budget_omits_rows_not_counts() {
        let content = "line\n".repeat(200);
        let diff = diff_added_file(&content);
        assert_eq!(diff.added_lines, 200);
        assert_eq!(diff.blocks[0].lines.len(), MAX_DIFF_LINES);
        assert_eq!(diff.omitted_lines, 200 - MAX_DIFF_LINES);
    }

    #[test]
    fn replaced_middle_keeps_line_numbers_on_both_sides() {
        let diff = diff_replaced("a\nb\nc\nd\ne\n", "a\nb\nX\nY\nd\ne\n");
        assert_eq!(diff.deleted_lines, 1);
        assert_eq!(diff.added_lines, 2);
        assert_eq!(diff.blocks.len(), 1);
        let block = &diff.blocks[0];
        assert_eq!(block.header, "@@ -3,1 +3,2 @@");
        assert_eq!(block.start_line_before, 3);
        assert_eq!(block.end_line_before, 3);
        assert_eq!(block.start_line_after, 3);
        assert_eq!(block.end_line_after, 4);
        assert_eq!(kinds(&diff), vec!["delete", "add", "add"]);
        assert_eq!(block.lines[0].old_line, Some(3));
        assert_eq!(block.lines[0].new_line, None);
        assert_eq!(block.lines[0].text, "c");
        assert_eq!(block.lines[1].old_line, None);
        assert_eq!(block.lines[1].new_line, Some(3));
        assert_eq!(block.lines[2].new_line, Some(4));
    }

    #[test]
    fn a_far_apart_edit_and_insert_are_counted_by_real_changes() {
        // 回归：顶部改一行 + 底部插入两行，曾经因为中间段长度不等被整段算成
        // 「先删后增」，把几十行以内的真实改动报成几百行（桌面版 +358/-356）。
        let mut before = String::new();
        let mut after = String::new();
        for index in 0..400 {
            before.push_str(&format!("line {index}\n"));
        }
        for index in 0..400 {
            if index == 10 {
                after.push_str("changed 10\n");
            } else {
                after.push_str(&format!("line {index}\n"));
            }
            if index == 300 {
                after.push_str("inserted a\ninserted b\n");
            }
        }
        let diff = diff_replaced(&before, &after);
        assert_eq!(diff.deleted_lines, 1);
        assert_eq!(diff.added_lines, 3);
        assert_eq!(diff.hunks, 2);
        assert_eq!(diff.omitted_lines, 0);

        let text = unified_diff("x.txt", &before, &after, &diff).text;
        assert!(text.contains("-line 10\n+changed 10"), "{text}");
        assert!(text.contains("+inserted a\n+inserted b"), "{text}");
        assert!(!text.contains("-line 200"), "{text}");
    }

    #[test]
    fn a_wide_literal_replace_reports_only_real_changes() {
        // 一次 old_string 覆盖整段、实际只改一行并在后面插入两行：计数按真实变更。
        let before: String = (0..50).map(|index| format!("row {index}\n")).collect();
        let mut after = String::new();
        for index in 0..50 {
            if index == 2 {
                after.push_str("ROW 2\n");
            } else {
                after.push_str(&format!("row {index}\n"));
            }
            if index == 40 {
                after.push_str("extra a\nextra b\n");
            }
        }
        let (diff, unavailable) =
            diff_literal_replaces(&before, &after, &[0], before.len(), &after);
        assert!(unavailable.is_none());
        assert_eq!(diff.deleted_lines, 1);
        assert_eq!(diff.added_lines, 3);
        assert_eq!(diff.hunks, 2);
    }

    #[test]
    fn equal_length_middles_split_into_per_line_blocks() {
        let diff = diff_replaced("a\nb\nc\nd\n", "a\nX\nc\nY\n");
        assert_eq!(diff.blocks.len(), 2);
        assert_eq!(diff.hunks, 2);
        assert_eq!(diff.added_lines, 2);
        assert_eq!(diff.deleted_lines, 2);
        assert_eq!(diff.blocks[1].start_line_before, 4);
        assert_eq!(diff.blocks[1].start_line_after, 4);
    }

    #[test]
    fn insertion_only_and_identical_content_behave() {
        let inserted = diff_replaced("a\nb\nc\n", "x\na\nb\nc\n");
        assert_eq!(inserted.added_lines, 1);
        assert_eq!(inserted.deleted_lines, 0);
        assert_eq!(inserted.blocks[0].header, "@@ -1,0 +1,1 @@");
        assert_eq!(kinds(&inserted), vec!["add"]);
        let unchanged = diff_replaced("a\nb\n", "a\nb\n");
        assert!(unchanged.blocks.is_empty());
        assert_eq!(unchanged.added_lines, 0);
        assert_eq!(unchanged.deleted_lines, 0);
        assert_eq!(unchanged.hunks, 0);
    }

    #[test]
    fn literal_replace_emits_one_block_per_occurrence_and_caps() {
        let before = "x\n".repeat(30);
        let after = "y\n".repeat(30);
        let offsets: Vec<usize> = (0..30).map(|index| index * 2).collect();
        let diff = replaces(&before, &after, &offsets, "x", "y");
        assert_eq!(diff.hunks, 30);
        assert_eq!(diff.blocks.len(), MAX_DIFF_BLOCKS);
        assert_eq!(diff.added_lines, 30);
        assert_eq!(diff.deleted_lines, 30);
        assert_eq!(diff.omitted_lines, 2 * (30 - MAX_DIFF_BLOCKS));
        assert_eq!(diff.blocks[0].header, "@@ -1,1 +1,1 @@");
        assert_eq!(
            diff.blocks[MAX_DIFF_BLOCKS - 1].start_line_before,
            MAX_DIFF_BLOCKS
        );
    }

    #[test]
    fn literal_replace_tracks_line_delta() {
        let before = "aa\nb\naa\nc\n";
        let after = "x\ny\nb\nx\ny\nc\n";
        let diff = replaces(before, after, &[0, 5], "aa", "x\ny");
        assert_eq!(diff.blocks.len(), 2);
        assert_eq!(diff.added_lines, 4);
        assert_eq!(diff.deleted_lines, 2);
        assert_eq!(diff.blocks[0].header, "@@ -1,1 +1,2 @@");
        assert_eq!(diff.blocks[1].header, "@@ -3,1 +4,2 @@");
        let last_add = diff.blocks[1].lines.last().expect("row");
        assert_eq!(last_add.text, "y");
        assert_eq!(last_add.new_line, Some(5));
    }

    #[test]
    fn literal_replace_of_empty_replacement_is_delete_only() {
        let diff = replaces("a\nb\n", "a\n", &[2], "b\n", "");
        assert_eq!(diff.blocks.len(), 1);
        assert_eq!(diff.added_lines, 0);
        assert_eq!(diff.deleted_lines, 1);
        assert_eq!(kinds(&diff), vec!["delete"]);
        assert_eq!(diff.blocks[0].header, "@@ -2,1 +2,0 @@");
    }

    #[test]
    fn literal_replace_skips_oversized_sources() {
        let big = "x\n".repeat(MAX_DIFF_SOURCE_BYTES / 2 + 1);
        let offsets = [0usize];
        let (diff, unavailable) = diff_literal_replaces(&big, "y\n", &offsets, 2, "y\n");
        assert_eq!(unavailable, Some(DIFF_UNAVAILABLE_OVERSIZE));
        assert!(diff.blocks.is_empty());
        assert_eq!(diff.added_lines, 0);
    }

    #[test]
    fn contract_keys_match_the_patch_family() {
        let diff = diff_replaced("a\nb\n", "a\nc\n");
        let mut map = Map::new();
        insert_diff_fields(&mut map, "update", "sample.txt", &diff, None);
        let fields = Value::Object(map);
        assert_eq!(
            sorted_keys(&fields),
            vec![
                "added",
                "added_lines",
                "changed_files",
                "deleted",
                "deleted_lines",
                "diff_lines_omitted",
                "files",
                "hunks_applied",
                "moved",
                "updated",
            ]
        );
        let file = &fields["files"][0];
        assert_eq!(
            sorted_keys(file),
            vec![
                "action",
                "added_lines",
                "deleted_lines",
                "diff_blocks",
                "diff_lines_omitted",
                "hunks",
                "path",
                "to_path"
            ]
        );
        assert_eq!(file["action"], json!("update"));
        assert_eq!(file["path"], json!("sample.txt"));
        assert!(file["to_path"].is_null());
        assert_eq!(file["hunks"], json!(1));
        let block = &file["diff_blocks"][0];
        assert_eq!(
            sorted_keys(block),
            vec![
                "end_line_after",
                "end_line_before",
                "header",
                "lines",
                "start_line_after",
                "start_line_before"
            ]
        );
        let line = &block["lines"][0];
        assert_eq!(
            sorted_keys(line),
            vec!["kind", "new_line", "old_line", "text"]
        );
        for emitted in block["lines"].as_array().expect("rows") {
            let kind = emitted["kind"].as_str().expect("kind");
            assert!(matches!(kind, "meta" | "add" | "delete"), "{kind}");
            let text = emitted["text"].as_str().expect("text");
            assert!(!text.starts_with('+') && !text.starts_with('-'));
        }
    }

    #[test]
    fn empty_diff_still_carries_the_contract_keys_without_blocks() {
        let mut map = Map::new();
        insert_diff_fields(&mut map, "update", "sample.txt", &LineDiff::default(), None);
        let fields = Value::Object(map);
        assert_eq!(fields["added_lines"], json!(0));
        assert_eq!(fields["changed_files"], json!(1));
        assert_eq!(fields["updated"], json!(1));
        assert_eq!(fields["added"], json!(0));
        assert!(fields["files"][0]["diff_blocks"]
            .as_array()
            .expect("array")
            .is_empty());
    }

    #[test]
    fn source_cap_rejects_oversized_sides() {
        let big = "x".repeat(MAX_DIFF_SOURCE_BYTES + 1);
        assert!(diff_source_too_large(&big, "small"));
        assert!(diff_source_too_large("small", &big));
        assert!(!diff_source_too_large("small", "other"));
    }
}
