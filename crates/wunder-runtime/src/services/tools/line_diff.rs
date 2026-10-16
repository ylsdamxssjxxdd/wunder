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

    /// 行正文预算：超预算的整行省略，只累加 `omitted_lines`。
    fn push_row(&mut self, rows: &mut Vec<DiffLine>, kind: &'static str, line: usize, text: &str) {
        if self.visible_lines >= MAX_DIFF_LINES
            || text.len() > MAX_DIFF_TEXT_BYTES.saturating_sub(self.text_bytes)
        {
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
    while region.old_len > 0
        && region.new_len > 0
        && old[region.old_start] == new[region.new_start]
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
    let last_segment = content.as_bytes().iter().filter(|byte| **byte == b'\n').count();
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
    let prefix = common_prefix_len(&old.texts, &new.texts);
    let suffix = common_suffix_len(&old.texts[prefix..], &new.texts[prefix..]);
    let old_mid = old.texts.len() - suffix - prefix;
    let new_mid = new.texts.len() - suffix - prefix;
    if old_mid == 0 && new_mid == 0 {
        return diff;
    }
    if old_mid == new_mid {
        let mut run: Option<Region> = None;
        for offset in 0..old_mid {
            if old.texts[prefix + offset] == new.texts[prefix + offset] {
                if let Some(region) = run.take() {
                    diff.push_region(&old.texts, &new.texts, region);
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
            diff.push_region(&old.texts, &new.texts, region);
        }
        return diff;
    }
    diff.push_region(
        &old.texts,
        &new.texts,
        Region {
            old_start: prefix,
            old_len: old_mid,
            new_start: prefix,
            new_len: new_mid,
        },
    );
    diff
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

fn file_entry(action: &str, path: &str, diff: &LineDiff) -> Value {
    json!({
        "action": action,
        "path": path,
        "to_path": Value::Null,
        "hunks": diff.hunks,
        "added_lines": diff.added_lines,
        "deleted_lines": diff.deleted_lines,
        "diff_lines_omitted": diff.omitted_lines,
        "diff_blocks": diff.blocks.iter().map(block_json).collect::<Vec<_>>(),
    })
}

/// 写入 `data` 的补丁家族键（单文件变更）。默认 `LineDiff` 表示「只有计数、
/// 没有块」，用于有界性守卫放弃 diff 的场景。
pub(super) fn insert_diff_fields(
    map: &mut Map<String, Value>,
    action: &str,
    path: &str,
    diff: &LineDiff,
) {
    map.insert("added_lines".to_string(), json!(diff.added_lines));
    map.insert("deleted_lines".to_string(), json!(diff.deleted_lines));
    map.insert("changed_files".to_string(), json!(1usize));
    map.insert("added".to_string(), json!(usize::from(action == "add")));
    map.insert("updated".to_string(), json!(usize::from(action == "update")));
    map.insert("deleted".to_string(), json!(usize::from(action == "delete")));
    map.insert("moved".to_string(), json!(0usize));
    map.insert("hunks_applied".to_string(), json!(diff.hunks));
    map.insert("diff_lines_omitted".to_string(), json!(diff.omitted_lines));
    map.insert("files".to_string(), json!([file_entry(action, path, diff)]));
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut keys: Vec<String> = value
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect();
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
        insert_diff_fields(&mut map, "update", "sample.txt", &diff);
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
        insert_diff_fields(&mut map, "update", "sample.txt", &LineDiff::default());
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
