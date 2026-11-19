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
//! 算法刻意保持最小且线性：公共前缀/后缀裁剪 → 中间整块「先删后增」；等长的
//! 中间段再按对齐位置切成若干区域；字面替换走已知字节区间，一个区间一个块。
//! 没有外部 diff 依赖，最坏 O(旧行数 + 新行数)，且行文本借用原文切片不复制。

use serde_json::{json, Map, Value};

/// 单次工具调用最多输出的块数；超出的块整块折进 `diff_lines_omitted`。
const MAX_DIFF_BLOCKS: usize = 12;
/// 可见行上限（与 `apply_patch_tool/preview.rs` 的每文件预算一致；本模块每次
/// 调用只处理一个文件，因此每文件即每次调用）。
const MAX_DIFF_LINES: usize = 80;
/// 行正文上限。
const MAX_DIFF_TEXT_BYTES: usize = 24 * 1024;
/// 允许参与 diff 的最大文本字节数；任一侧到界即放弃 diff（只给计数）。
pub(super) const MAX_DIFF_SOURCE_BYTES: usize = 512 * 1024;

/// `diff_unavailable` 的短原因串取值集合。
pub(super) const DIFF_UNAVAILABLE_OVERSIZE: &str = "oversize";
pub(super) const DIFF_UNAVAILABLE_BINARY: &str = "binary";
pub(super) const DIFF_UNAVAILABLE_UNREADABLE: &str = "unreadable";

/// `diff_omitted` 的短原因串：单行太长，正文放不下。
pub(super) const OMITTED_LINE_TOO_LONG: &str = "line_too_long";
/// `diff_omitted` 的短原因串：行数/块数预算用完。
pub(super) const OMITTED_BUDGET: &str = "budget";
/// `diff_omitted` 的短原因串：新建文件的正文就是调用方刚发出去的内容，
/// 有意不回灌（体积与文件大小无关）。
pub(super) const OMITTED_NEW_FILE: &str = "new_file";

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

fn common_prefix_len(old: &[&str], new: &[&str]) -> usize {
    old.iter().zip(new).take_while(|(a, b)| a == b).count()
}

fn common_suffix_len(old: &[&str], new: &[&str]) -> usize {
    old.iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
}

/// 区域内再裁公共首尾行，只留下真正变化的行。
fn trim_region(old: &[&str], new: &[&str], mut region: Region) -> Region {
    region = clamp_region(old, new, region);
    while region.old_len > 0 && region.new_len > 0 && old[region.old_start] == new[region.new_start]
    {
        region.old_start += 1;
        region.old_len -= 1;
        region.new_start += 1;
        region.new_len -= 1;
    }
    while region.old_len > 0
        && region.new_len > 0
        && old[region.old_start + region.old_len - 1] == new[region.new_start + region.new_len - 1]
    {
        region.old_len -= 1;
        region.new_len -= 1;
    }
    region
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

/// 覆盖写：公共前缀/后缀裁剪后，中间整块先删后增；等长中间段按对齐位置切块。
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
    let prefix = common_prefix_len(old, new);
    let suffix = common_suffix_len(&old[prefix..], &new[prefix..]);
    let old_mid = old.len() - suffix - prefix;
    let new_mid = new.len() - suffix - prefix;
    if old_mid == 0 && new_mid == 0 {
        return Vec::new();
    }
    if old_mid != new_mid {
        return vec![Region {
            old_start: prefix,
            old_len: old_mid,
            new_start: prefix,
            new_len: new_mid,
        }];
    }
    let mut regions = Vec::new();
    let mut run: Option<Region> = None;
    for offset in 0..old_mid {
        if old[prefix + offset] == new[prefix + offset] {
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
                    old_start: prefix + offset,
                    old_len: 1,
                    new_start: prefix + offset,
                    new_len: 1,
                })
            }
        }
    }
    if let Some(region) = run {
        regions.push(region);
    }
    regions
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
        let region = trim_region(
            &old.texts,
            &new.texts,
            Region {
                old_start,
                old_len,
                new_start,
                new_len,
            },
        );
        diff.push_region(&old.texts, &new.texts, region);
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
        "diff_omitted": if unknown { Value::Null } else { json!(diff.omission()) },
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
    // 省了就要说：只给 `---`/`+++` 头的 diff 会被读成「没有改动」。
    map.insert("diff_omitted".to_string(), json!(diff.omission()));
    map.insert(
        "files".to_string(),
        json!([file_entry(action, path, diff, unknown)]),
    );
}

/// 每个变更块前后各带几行上下文。
const UNIFIED_CONTEXT_LINES: usize = 2;

/// 模型面 unified diff 的硬预算（字节）。
///
/// 写类结果不参与观察层压缩（见 `orchestrator::tool_result_payload`），所以这一层
/// 必须自己兜底：整份文件回灌只会在下一次编辑前就撑爆上下文。
pub(super) const MAX_UNIFIED_DIFF_BYTES: usize = 32 * 1024;

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

/// 新建文件的 diff 正文就是调用方刚发出去的内容，回灌没有信息增量：只给一个
/// 与 unified diff 同形的三行说明，体积与文件大小无关。
pub(super) fn new_file_notice(path: &str, lines: usize) -> String {
    format!(
        "--- a/{path}\n+++ b/{path}\nnew file: {lines} lines written \
         (diff body omitted: content is what this call sent)"
    )
}

/// 把（旧文本，新文本）渲染成 unified diff 文本：`---`/`+++`/`@@` + 上下文 + `-`/`+`。
///
/// 写类工具（写入文件、文本编辑）的回执靠这段文本确认「自己刚写进去的到底是
/// 什么」，所以它不受观察层的通用截断；但它自己有 [`MAX_UNIFIED_DIFF_BYTES`]
/// 的结构预算：整块放得下就完整输出，放不下就把该 hunk 折成「头 + 省略行数」，
/// 计数始终准确，模型仍能看出改了哪里、改了多少。
///
/// 源文本超过 `MAX_DIFF_SOURCE_BYTES` 时调用方本就给出 `diff_unavailable`，
/// 这里不再重算。
pub(super) fn unified_diff(path: &str, before: &str, after: &str, diff: &LineDiff) -> UnifiedDiff {
    let old_index = build_line_index(before);
    let new_index = build_line_index(after);
    let old = &old_index.texts;
    let new = &new_index.texts;
    let regions: Vec<Region> = if diff.omission().is_none() || diff_source_too_large(before, after)
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
        if used + needed <= content_budget {
            used += needed;
            out.push(header);
            push_lines(&mut out, " ", pre);
            push_lines(&mut out, "-", removed);
            push_lines(&mut out, "+", added);
            push_lines(&mut out, " ", post);
            emitted_old_end = post_to.max(post_from);
            continue;
        }

        // 整块放不下：留一行「头 + 省略行数」，模型仍能定位改动区间与规模。
        let folded = format!(
            "{header} ... {old_len} removed / {new_len} added line(s) omitted (diff budget)"
        );
        if used + line_bytes(&folded) <= content_budget {
            used += line_bytes(&folded);
            out.push(folded);
            omitted_lines += old_len + new_len;
            emitted_old_end = post_to.max(post_from);
            continue;
        }
        // 连折叠后的一行都放不下：后面的块只记账，不再输出。
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

/// 模型面文本被省掉时，`diff_omitted` 也要给出原因：留着 null 会和
/// `unified_diff_truncated: true` 自相矛盾，读者只能猜。
pub(super) fn mark_text_omission(map: &mut Map<String, Value>, reason: &'static str) {
    if map.get("diff_omitted").is_none_or(Value::is_null) {
        map.insert("diff_omitted".to_string(), json!(reason));
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
    fn a_single_huge_line_is_folded_to_a_marker_instead_of_blowing_the_budget() {
        // 200 KB 单行：结构化块放不进正文预算，模型面文本也只能折叠；
        // 但计数必须准确，且总字节不许越过硬预算。
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
        assert!(
            rendered.text.contains("@@ -1,1 +1,1 @@"),
            "{}",
            &rendered.text[..64]
        );
        assert!(rendered
            .text
            .contains("1 removed / 1 added line(s) omitted"));
        assert!(
            !rendered.text.contains(&"x".repeat(1024)),
            "超长行不进上下文之外"
        );
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
    fn unified_diff_keeps_every_changed_line_past_the_block_budget() {
        // 200 行全改：结构化块按 12 块/80 行预算折叠，模型回执必须是完整 diff。
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
        let text = unified_diff("x.txt", &before, &after, &diff).text;
        assert!(text.contains("@@ -1,200 +1,200 @@"), "{}", &text[..64]);
        for index in 0..200 {
            let removed = format!("-line {index}");
            let added = format!("+line {index}!");
            assert!(text.lines().any(|line| line == removed), "{index}");
            assert!(text.lines().any(|line| line == added), "{index}");
        }
    }

    #[test]
    fn unified_diff_folds_past_the_byte_budget_and_keeps_counts_exact() {
        // 5000 行里隔行改：2500 个 hunk，完整渲染约 200 KB，必须折叠到预算内，
        // 但「省了多少」要准确，模型据此才知道文件到底改动了多大。
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
            rendered.text.len() <= MAX_UNIFIED_DIFF_BYTES,
            "{}",
            rendered.text.len()
        );
        assert_eq!(rendered.omitted_lines, rendered.omitted_hunks * 2);
        assert!(rendered.text.contains("hunk(s) / "));
        // 折叠后仍要看得出改在哪：至少保留了一部分真实 hunk 头。
        assert!(rendered.text.contains("@@ -1,1 +1,1 @@"));
    }

    #[test]
    fn a_fifty_kilobyte_edit_is_already_folded() {
        // 3000 行 ≈ 51 KB：旧口径（64 KB）会整份回灌，新口径必须折叠。
        let mut before = String::new();
        for index in 0..3000 {
            before.push_str(&format!("line {:05} value\n", index));
        }
        let after = "only line\n";
        let diff = diff_replaced(&before, after);
        let rendered = unified_diff("editme.txt", &before, after, &diff);
        assert!(rendered.truncated());
        assert!(rendered.text.len() <= MAX_UNIFIED_DIFF_BYTES);
        assert!(rendered
            .text
            .contains("3000 removed / 1 added line(s) omitted (diff budget)"));
    }

    #[test]
    fn a_whole_file_rewrite_folds_into_one_hunk_header() {
        let before = "old line\n".repeat(4000);
        let after = "new line\n".repeat(4000);
        let diff = diff_replaced(&before, &after);
        let rendered = unified_diff("rewrite.txt", &before, &after, &diff);
        assert!(rendered.truncated());
        assert_eq!(rendered.omitted_hunks, 0);
        assert_eq!(rendered.omitted_lines, 8000);
        assert!(rendered.text.len() <= MAX_UNIFIED_DIFF_BYTES);
        assert!(
            rendered
                .text
                .contains("@@ -1,4000 +1,4000 @@ ... 4000 removed / 4000 added line(s) omitted"),
            "{}",
            &rendered.text[..96]
        );
        assert!(
            rendered
                .text
                .contains("... 8000 line(s) omitted (diff budget)"),
            "{}",
            &rendered.text[rendered.text.len().saturating_sub(64)..]
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
        assert_eq!(diff.blocks[11].start_line_before, 12);
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
                "diff_omitted",
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
                "diff_omitted",
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
