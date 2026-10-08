use super::{ChunkContextFailureDetail, ChunkContextFailureKind, ChunkLineKind, UpdateChunk};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UpdateChunkEffect {
    pub(crate) has_context: bool,
    pub(crate) context_lines: usize,
    pub(crate) has_add: bool,
    pub(crate) has_delete: bool,
    pub(crate) old_len: usize,
    pub(crate) new_len: usize,
    pub(crate) looks_like_missing_prefixes: bool,
}

#[cfg(test)]
pub(super) enum ChunkRangeSearchResult {
    Found((usize, usize)),
    NotFound,
    Ambiguous { matches: usize },
}

#[derive(Debug, Clone)]
struct ChunkSearchPlan {
    search_start: usize,
    anchor: Option<String>,
    anchor_found: bool,
}

#[derive(Debug, Clone)]
struct PartialMatchWindow {
    start: usize,
    matched_lines: usize,
    total_lines: usize,
    diffs: Vec<(usize, String, String)>,
}

#[derive(Debug, Clone, Copy, Default)]
struct ChunkShape {
    leading_context: usize,
    trailing_context: usize,
    has_add: bool,
    has_delete: bool,
}

#[cfg(test)]
pub(super) fn find_chunk_range(
    source_lines: &[String],
    cursor: usize,
    chunk: &UpdateChunk,
    old_lines: &[String],
) -> ChunkRangeSearchResult {
    let len = source_lines.len();
    let search_plan = derive_chunk_search_plan(source_lines, cursor, chunk);
    let search_start = search_plan.search_start;

    if old_lines.is_empty() {
        let start = if chunk.end_of_file { len } else { search_start };
        return ChunkRangeSearchResult::Found((start, start));
    }
    if old_lines.len() > len {
        return ChunkRangeSearchResult::NotFound;
    }

    let max_start = len.saturating_sub(old_lines.len());
    let primary_matches = collect_chunk_match_starts(
        source_lines,
        old_lines,
        search_start,
        max_start,
        chunk.end_of_file,
    );
    if let Some(start) = primary_matches.first().copied() {
        let end = start + old_lines.len();
        return ChunkRangeSearchResult::Found((start, end));
    }

    // Fallback: strip display line numbers from old_lines and retry matching.
    let stripped_old: Vec<String> = old_lines
        .iter()
        .map(|line| super::parser::strip_display_line_number(line).unwrap_or_else(|| line.clone()))
        .collect();
    if stripped_old != old_lines {
        let stripped_matches = collect_chunk_match_starts(
            source_lines,
            &stripped_old,
            search_start,
            max_start,
            chunk.end_of_file,
        );
        if let Some(start) = stripped_matches.first().copied() {
            let end = start + stripped_old.len();
            return ChunkRangeSearchResult::Found((start, end));
        }
        if search_start > 0 {
            let fallback_end = search_start.saturating_sub(1).min(max_start);
            let fallback_stripped = collect_chunk_match_starts(
                source_lines,
                &stripped_old,
                0,
                fallback_end,
                chunk.end_of_file,
            );
            match fallback_stripped.len() {
                0 => {}
                1 => {
                    let start = fallback_stripped[0];
                    let end = start + stripped_old.len();
                    return ChunkRangeSearchResult::Found((start, end));
                }
                matches => return ChunkRangeSearchResult::Ambiguous { matches },
            }
        }
    }

    // Fallback: fuzzy match by normalizing whitespace (trim leading/trailing).
    let fuzzy_old: Vec<String> = old_lines
        .iter()
        .map(|line| line.trim().to_string())
        .collect();
    let fuzzy_source: Vec<String> = source_lines
        .iter()
        .map(|line| line.trim().to_string())
        .collect();
    if fuzzy_old != old_lines {
        let fuzzy_max_start = len.saturating_sub(old_lines.len());
        let fuzzy_matches = collect_fuzzy_match_starts(
            &fuzzy_source,
            &fuzzy_old,
            search_start,
            fuzzy_max_start,
            chunk.end_of_file,
        );
        if let Some(start) = fuzzy_matches.first().copied() {
            let end = start + old_lines.len();
            return ChunkRangeSearchResult::Found((start, end));
        }
        if search_start > 0 {
            let fuzzy_fallback_end = search_start.saturating_sub(1).min(fuzzy_max_start);
            let fuzzy_fallback = collect_fuzzy_match_starts(
                &fuzzy_source,
                &fuzzy_old,
                0,
                fuzzy_fallback_end,
                chunk.end_of_file,
            );
            match fuzzy_fallback.len() {
                0 => {}
                1 => {
                    let start = fuzzy_fallback[0];
                    let end = start + old_lines.len();
                    return ChunkRangeSearchResult::Found((start, end));
                }
                matches => return ChunkRangeSearchResult::Ambiguous { matches },
            }
        }
    }

    if search_start == 0 {
        return ChunkRangeSearchResult::NotFound;
    }

    let fallback_end = search_start.saturating_sub(1).min(max_start);
    let fallback_matches =
        collect_chunk_match_starts(source_lines, old_lines, 0, fallback_end, chunk.end_of_file);
    match fallback_matches.len() {
        0 => ChunkRangeSearchResult::NotFound,
        1 => {
            let start = fallback_matches[0];
            let end = start + old_lines.len();
            ChunkRangeSearchResult::Found((start, end))
        }
        matches => ChunkRangeSearchResult::Ambiguous { matches },
    }
}

fn derive_chunk_search_plan(
    source_lines: &[String],
    cursor: usize,
    chunk: &UpdateChunk,
) -> ChunkSearchPlan {
    let len = source_lines.len();
    let mut search_start = cursor.min(len);
    let mut anchor = None;
    let mut anchor_found = false;

    if let Some(raw_anchor) = chunk
        .change_context
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        anchor = Some(raw_anchor.to_string());
        if let Some(anchor_index) = source_lines
            .iter()
            .enumerate()
            .skip(search_start)
            .find_map(|(idx, line)| line.contains(raw_anchor).then_some(idx))
        {
            search_start = anchor_index;
            anchor_found = true;
        }
    }

    ChunkSearchPlan {
        search_start,
        anchor,
        anchor_found,
    }
}

fn analyze_chunk_shape(chunk: &UpdateChunk) -> ChunkShape {
    let mut shape = ChunkShape::default();
    let mut seen_non_context = false;
    for line in &chunk.lines {
        match line.kind {
            ChunkLineKind::Context => {
                if seen_non_context {
                    shape.trailing_context += 1;
                } else {
                    shape.leading_context += 1;
                }
            }
            ChunkLineKind::Add => {
                shape.has_add = true;
                seen_non_context = true;
            }
            ChunkLineKind::Delete => {
                shape.has_delete = true;
                seen_non_context = true;
            }
        }
    }
    shape
}

pub(super) fn analyze_update_chunk_effect(chunk: &UpdateChunk) -> UpdateChunkEffect {
    let mut effect = UpdateChunkEffect::default();
    let mut previous_context_line: Option<String> = None;
    for line in &chunk.lines {
        match line.kind {
            ChunkLineKind::Context => {
                effect.has_context = true;
                effect.context_lines = effect.context_lines.saturating_add(1);
                if let Some(previous) = previous_context_line.as_deref() {
                    if looks_like_missing_change_prefix_pair(previous, &line.text) {
                        effect.looks_like_missing_prefixes = true;
                    }
                }
                previous_context_line = Some(line.text.clone());
            }
            ChunkLineKind::Add => {
                effect.has_add = true;
                effect.new_len = effect.new_len.saturating_add(1);
                previous_context_line = None;
            }
            ChunkLineKind::Delete => {
                effect.has_delete = true;
                effect.old_len = effect.old_len.saturating_add(1);
                previous_context_line = None;
            }
        }
    }
    effect
}

fn looks_like_missing_change_prefix_pair(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() || left == right {
        return false;
    }
    let left_lhs = left.split_once('=').map(|(lhs, _)| lhs.trim());
    let right_lhs = right.split_once('=').map(|(lhs, _)| lhs.trim());
    if let (Some(left_lhs), Some(right_lhs)) = (left_lhs, right_lhs) {
        return !left_lhs.is_empty() && left_lhs == right_lhs;
    }
    false
}

pub(super) fn build_patch_no_effect_hint(
    no_effect_chunk_effects: &[Vec<UpdateChunkEffect>],
) -> (String, String) {
    let effects = no_effect_chunk_effects.iter().flatten().collect::<Vec<_>>();
    let has_missing_prefixes = effects
        .iter()
        .any(|effect| effect.looks_like_missing_prefixes);
    if has_missing_prefixes {
        return (
            "检测到这次补丁里有一些行看起来像同一个赋值语句的旧值/新值，但它们都被当成了普通上下文行。通常是你忘了给旧行加 `-`、给新行加 `+`。请把真正要替换的两行明确写成 `-旧行` 和 `+新行`。".to_string(),
            "Some lines in this patch look like old/new versions of the same assignment, but they were both treated as ordinary context lines. This usually means the old line is missing `-` and the new line is missing `+`. Rewrite the real replacement as `-old_line` and `+new_line`.".to_string(),
        );
    }

    let has_context_only_hunk = effects
        .iter()
        .any(|effect| effect.has_context && !effect.has_add && !effect.has_delete);
    if has_context_only_hunk {
        return (
            "这次补丁只有上下文，没有任何真正的新增/删除行。请确认每个 Update File 至少包含一对 `-旧行` / `+新行`，或者把真正改动并回同一个 hunk。".to_string(),
            "This patch only contains context and no real add/delete lines. Make sure each Update File has at least one `-old_line` / `+new_line` pair, or merge the real edit back into the same hunk.".to_string(),
        );
    }

    let has_same_add_delete = effects
        .iter()
        .any(|effect| effect.has_add && effect.has_delete && effect.old_len == effect.new_len);
    if has_same_add_delete {
        return (
            "这次补丁里的 `-` 和 `+` 行实际改回了同一个内容，所以文件没有变化。请重新读取最新文件，确认删除行和新增行确实不同后再提交。".to_string(),
            "The `-` and `+` lines in this patch effectively changed back to the same content, so nothing changed on disk. Re-read the latest file and ensure the deleted and added lines are actually different before retrying.".to_string(),
        );
    }

    (
        "这通常表示 Update File 里的 `-` 与 `+` 内容实际相同，或补丁只重复写回原内容。请重新读取文件，确认新增与删除行确实不同后再重试。".to_string(),
        "This usually means the Update File `-` and `+` lines are effectively identical, or the patch only rewrote the original content. Re-read the file and ensure added and deleted lines are actually different before retrying.".to_string(),
    )
}

pub(crate) fn build_context_not_found_hint(
    source_lines: &[String],
    old_lines: &[String],
    cursor: usize,
    chunk: &UpdateChunk,
    failure: Option<&ChunkContextFailureDetail>,
) -> (String, String) {
    let search_plan = derive_chunk_search_plan(source_lines, cursor, chunk);
    let chunk_shape = analyze_chunk_shape(chunk);
    let expected_preview = format_numbered_preview(old_lines, 0, 4);
    let nearby_start = search_plan.search_start.saturating_sub(2);
    let nearby_preview = format_numbered_preview(source_lines, nearby_start, 6);
    let best_partial =
        find_best_partial_match_window(source_lines, old_lines, search_plan.search_start);

    let has_context_delete_dup = chunk.lines.windows(2).any(|pair| {
        matches!(pair[0].kind, ChunkLineKind::Context)
            && matches!(pair[1].kind, ChunkLineKind::Delete)
            && pair[0].text == pair[1].text
    });

    let zh_anchor = match search_plan.anchor.as_deref() {
        Some(anchor) if search_plan.anchor_found => {
            format!(
                "@@ 锚点“{anchor}”命中第 {} 行。",
                search_plan.search_start + 1
            )
        }
        Some(anchor) => format!(
            "@@ 锚点“{anchor}”未命中，已从第 {} 行附近继续搜索。",
            search_plan.search_start + 1
        ),
        None => format!(
            "未提供 @@ 锚点，从第 {} 行附近开始搜索。",
            search_plan.search_start + 1
        ),
    };
    let en_anchor = match search_plan.anchor.as_deref() {
        Some(anchor) if search_plan.anchor_found => {
            format!(
                "@@ anchor \"{anchor}\" matched at line {}.",
                search_plan.search_start + 1
            )
        }
        Some(anchor) => format!(
            "@@ anchor \"{anchor}\" was not found; continued searching near line {}.",
            search_plan.search_start + 1
        ),
        None => format!(
            "No @@ anchor was provided; started searching near line {}.",
            search_plan.search_start + 1
        ),
    };

    let zh_partial = if let Some(window) = best_partial.as_ref() {
        format!(
            "最接近片段位于第 {} 行，匹配 {}/{} 行。差异示例：\n{}",
            window.start + 1,
            window.matched_lines,
            window.total_lines,
            format_partial_mismatch_examples(window)
        )
    } else {
        "全文件中未找到可部分匹配的片段。".to_string()
    };
    let en_partial = if let Some(window) = best_partial.as_ref() {
        format!(
            "Nearest window starts at line {} with {}/{} lines matched. Mismatch samples:\n{}",
            window.start + 1,
            window.matched_lines,
            window.total_lines,
            format_partial_mismatch_examples(window)
        )
    } else {
        "No partially matching window was found in the file.".to_string()
    };

    let zh_dup_warn = if has_context_delete_dup {
        "检测到上下文行与紧随的删除行内容重复：要删除/替换的那一行只能保留为 `-旧行`，不要再把同一行额外写成空格上下文行。请删掉重复的上下文行。\n"
    } else {
        ""
    };
    let en_dup_warn = if has_context_delete_dup {
        "Detected a context line duplicated with the following delete line: a line being deleted/replaced should appear only as `-old_line`, not also as a space-context line. Remove the redundant context line.\n"
    } else {
        ""
    };
    let (zh_fix, en_fix) = match failure.map(|detail| detail.kind) {
        Some(ChunkContextFailureKind::DuplicateAnchorAfterContextOnlyChunk) => (
            "检测到前一个变更块是空 hunk，并且当前变更块复用了同一组锚点。通常是多写了一个只有上下文的 @@ 块，导致前一个空块先占用了匹配位置。请删除前一个空 hunk，或把当前插入/替换内容并入同一个 hunk 后再重试。\n".to_string(),
            "The previous chunk appears to be an empty hunk, and this chunk reuses the same anchor lines. This usually means an extra context-only @@ block consumed the match position first. Remove the earlier empty hunk, or merge the insertion/replacement into a single hunk and retry.\n".to_string(),
        ),
        _ => (String::new(), String::new()),
    };
    let (zh_insert_fix, en_insert_fix) = if chunk_shape.has_add
        && !chunk_shape.has_delete
        && chunk_shape.trailing_context == 0
    {
        (
            "检测到这是一个纯插入 hunk，而且插入内容后面没有稳定的尾部上下文。若当前位置附近存在重复结构，模型很容易把插入点放错。请在插入内容后再保留 1-2 行未修改原文，作为尾部锚点；或者把插入内容并回前后已有上下文所在的同一个 hunk。\n".to_string(),
            "This appears to be an insertion-only hunk with no stable trailing context after the inserted lines. If similar structure repeats nearby, the insertion point is easy to misplace. Keep 1-2 unchanged lines after the insertion as a trailing anchor, or merge the insertion back into the surrounding hunk.\n".to_string(),
        )
    } else if chunk_shape.has_add && !chunk_shape.has_delete && chunk_shape.leading_context == 0 {
        (
            "检测到这是一个纯插入 hunk，而且插入内容前面没有稳定的头部上下文。请在插入内容前补 1-2 行未修改原文，作为头部锚点；或者把插入内容并回前后已有上下文所在的同一个 hunk。\n".to_string(),
            "This appears to be an insertion-only hunk with no stable leading context before the inserted lines. Add 1-2 unchanged lines before the insertion as a leading anchor, or merge the insertion back into the surrounding hunk.\n".to_string(),
        )
    } else {
        (String::new(), String::new())
    };

    let zh_hint = format!(
        "{zh_dup_warn}{zh_fix}{zh_insert_fix}请直接读取相关片段并重建补丁，或补充更稳定的 @@ 上下文；不要整文件盲目重读。\n{zh_anchor}\n期望旧片段（前 4 行）：\n{expected_preview}\n邻近源码（从第 {} 行起）：\n{nearby_preview}\n{zh_partial}",
        nearby_start + 1
    );
    let en_hint = format!(
        "{en_dup_warn}{en_fix}{en_insert_fix}Read only the relevant excerpt and rebuild the patch, or add more stable @@ context; avoid blindly re-reading the whole file.\n{en_anchor}\nExpected old snippet (first 4 lines):\n{expected_preview}\nNearby source (starting at line {}):\n{nearby_preview}\n{en_partial}",
        nearby_start + 1
    );
    (zh_hint, en_hint)
}

fn find_best_partial_match_window(
    source_lines: &[String],
    old_lines: &[String],
    preferred_start: usize,
) -> Option<PartialMatchWindow> {
    if old_lines.is_empty() || source_lines.len() < old_lines.len() {
        return None;
    }
    let max_start = source_lines.len() - old_lines.len();
    let mut best: Option<PartialMatchWindow> = None;
    let mut best_distance = usize::MAX;
    for start in 0..=max_start {
        let mut matched_lines = 0usize;
        let mut diffs = Vec::new();
        for (offset, expected) in old_lines.iter().enumerate() {
            let actual = &source_lines[start + offset];
            if expected == actual {
                matched_lines += 1;
                continue;
            }
            if diffs.len() < 3 {
                diffs.push((
                    offset,
                    truncate_for_hint(expected, 120),
                    truncate_for_hint(actual, 120),
                ));
            }
        }
        if matched_lines == 0 {
            continue;
        }
        let distance = start.abs_diff(preferred_start);
        let should_replace = match best.as_ref() {
            Some(existing) => {
                matched_lines > existing.matched_lines
                    || (matched_lines == existing.matched_lines && distance < best_distance)
            }
            None => true,
        };
        if should_replace {
            best_distance = distance;
            best = Some(PartialMatchWindow {
                start,
                matched_lines,
                total_lines: old_lines.len(),
                diffs,
            });
        }
    }
    best
}

fn format_partial_mismatch_examples(window: &PartialMatchWindow) -> String {
    if window.diffs.is_empty() {
        return "- (all lines matched)".to_string();
    }
    window
        .diffs
        .iter()
        .map(|(offset, expected, actual)| {
            let line_no = window.start + offset + 1;
            format!("L{line_no}\n  expected: {expected}\n  actual:   {actual}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_numbered_preview(lines: &[String], start: usize, max_lines: usize) -> String {
    if lines.is_empty() || start >= lines.len() {
        return "(empty)".to_string();
    }
    let end = (start + max_lines).min(lines.len());
    lines[start..end]
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            let line_no = start + idx + 1;
            format!("{line_no:>4}: {}", truncate_for_hint(line, 120))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_for_hint(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        return line.to_string();
    }
    let truncated = line.chars().take(max_chars).collect::<String>();
    format!("{truncated}...")
}

#[cfg(test)]
fn collect_chunk_match_starts(
    source_lines: &[String],
    old_lines: &[String],
    start: usize,
    end: usize,
    end_of_file: bool,
) -> Vec<usize> {
    if old_lines.is_empty() || start > end {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for line_start in start..=end {
        let line_end = line_start + old_lines.len();
        if end_of_file && line_end != source_lines.len() {
            continue;
        }
        if source_lines[line_start..line_end] == *old_lines {
            matches.push(line_start);
        }
    }
    matches
}

#[cfg(test)]
fn collect_fuzzy_match_starts(
    fuzzy_source: &[String],
    fuzzy_old: &[String],
    start: usize,
    end: usize,
    end_of_file: bool,
) -> Vec<usize> {
    if fuzzy_old.is_empty() || start > end {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for line_start in start..=end {
        let line_end = line_start + fuzzy_old.len();
        if end_of_file && line_end != fuzzy_source.len() {
            continue;
        }
        if fuzzy_source[line_start..line_end] == *fuzzy_old {
            matches.push(line_start);
        }
    }
    matches
}
