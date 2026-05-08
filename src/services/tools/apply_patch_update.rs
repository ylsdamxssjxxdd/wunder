use anyhow::Result;

use super::apply_patch_tool::{
    build_context_not_found_hint, ensure_patch_not_cancelled_probe, patch_error_with_hint,
    split_lines, ChunkLine, ChunkLineKind, FileDiffBlock, FileDiffLine, PatchCancelProbe,
    UpdateChunk, PATCH_CANCEL_CHECK_INTERVAL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChunkContextFailureKind {
    Generic,
    DuplicateAnchorAfterContextOnlyChunk,
}

#[derive(Debug, Clone)]
pub(super) struct ChunkContextFailureDetail {
    pub(super) kind: ChunkContextFailureKind,
}

#[derive(Debug, Clone)]
struct LineReplacement {
    start: usize,
    old_len: usize,
    new_lines: Vec<String>,
    display_lines: Vec<ChunkLine>,
}

fn build_update_diff_blocks(replacements: &[LineReplacement]) -> Vec<FileDiffBlock> {
    let mut blocks = Vec::new();
    let mut line_delta = 0isize;

    for replacement in replacements {
        let start_before = replacement.start + 1;
        let end_before = if replacement.old_len == 0 {
            replacement.start
        } else {
            replacement.start + replacement.old_len
        };
        let after_start_zero = ((replacement.start as isize) + line_delta).max(0) as usize;
        let start_after = after_start_zero + 1;
        let end_after = if replacement.new_lines.is_empty() {
            after_start_zero
        } else {
            after_start_zero + replacement.new_lines.len()
        };
        let mut lines = Vec::with_capacity(replacement.display_lines.len());
        let mut old_cursor = start_before;
        let mut new_cursor = start_after;

        for line in &replacement.display_lines {
            match line.kind {
                ChunkLineKind::Context => {
                    lines.push(FileDiffLine {
                        kind: "meta",
                        old_line: Some(old_cursor),
                        new_line: Some(new_cursor),
                        text: line.text.clone(),
                    });
                    old_cursor += 1;
                    new_cursor += 1;
                }
                ChunkLineKind::Delete => {
                    lines.push(FileDiffLine {
                        kind: "delete",
                        old_line: Some(old_cursor),
                        new_line: None,
                        text: line.text.clone(),
                    });
                    old_cursor += 1;
                }
                ChunkLineKind::Add => {
                    lines.push(FileDiffLine {
                        kind: "add",
                        old_line: None,
                        new_line: Some(new_cursor),
                        text: line.text.clone(),
                    });
                    new_cursor += 1;
                }
            }
        }

        blocks.push(FileDiffBlock {
            header: format!(
                "@@ -{},{} +{},{} @@",
                start_before,
                replacement.old_len,
                start_after,
                replacement.new_lines.len()
            ),
            start_line_before: start_before,
            end_line_before: end_before,
            start_line_after: start_after,
            end_line_after: end_after,
            lines,
        });

        line_delta += replacement.new_lines.len() as isize - replacement.old_len as isize;
    }

    blocks
}

fn compute_chunk_replacements(
    source_lines: &[String],
    path: &str,
    chunks: &[UpdateChunk],
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<Vec<LineReplacement>> {
    let mut replacements = Vec::new();
    let mut line_index = 0usize;
    let mut previous_context_only_chunk: Option<(usize, usize, Vec<String>)> = None;

    for (index, chunk) in chunks.iter().enumerate() {
        if index % PATCH_CANCEL_CHECK_INTERVAL == 0 {
            if let Some(probe) = cancel_probe {
                ensure_patch_not_cancelled_probe(probe)?;
            }
        }

        if let Some(raw_anchor) = chunk
            .change_context
            .as_deref()
            .map(str::trim)
            .filter(|text: &&str| !text.is_empty())
        {
            let anchor_lines = vec![raw_anchor.to_string()];
            let Some(anchor_index) = seek_sequence(source_lines, &anchor_lines, line_index, false)
            else {
                let synthetic_chunk = UpdateChunk {
                    change_context: Some(raw_anchor.to_string()),
                    lines: vec![ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: raw_anchor.to_string(),
                    }],
                    end_of_file: false,
                };
                let (hint_zh, hint_en) = build_context_not_found_hint(
                    source_lines,
                    &anchor_lines,
                    line_index,
                    &synthetic_chunk,
                    Some(&ChunkContextFailureDetail {
                        kind: ChunkContextFailureKind::Generic,
                    }),
                );
                return Err(patch_error_with_hint(
                    "PATCH_CONTEXT_NOT_FOUND",
                    format!(
                        "补丁应用失败：{path} 第 {} 个变更块找不到 @@ 锚点 {}",
                        index + 1,
                        raw_anchor
                    ),
                    format!(
                        "Patch apply failed: chunk {} in {} cannot find @@ anchor {}",
                        index + 1,
                        path,
                        raw_anchor
                    ),
                    hint_zh,
                    hint_en,
                ));
            };
            line_index = anchor_index + 1;
        }

        let old_lines = chunk
            .lines
            .iter()
            .filter(|line| line.kind != ChunkLineKind::Add)
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        let mut new_lines = chunk
            .lines
            .iter()
            .filter(|line| line.kind != ChunkLineKind::Delete)
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        let has_actual_change = chunk
            .lines
            .iter()
            .any(|line| !matches!(line.kind, ChunkLineKind::Context));

        if !has_actual_change {
            let pattern = if old_lines.is_empty() {
                chunk.lines.iter().map(|line| line.text.clone()).collect()
            } else {
                old_lines.clone()
            };
            previous_context_only_chunk = Some((index, line_index, pattern));
            continue;
        }

        if old_lines.is_empty() {
            previous_context_only_chunk = None;
            replacements.push(LineReplacement {
                start: source_lines.len(),
                old_len: 0,
                new_lines,
                display_lines: chunk.lines.clone(),
            });
            continue;
        }

        let mut pattern = old_lines;
        let mut found = seek_sequence(source_lines, &pattern, line_index, chunk.end_of_file);
        if found.is_none() && pattern.last().is_some_and(|line: &String| line.is_empty()) {
            pattern.pop();
            if new_lines
                .last()
                .is_some_and(|line: &String| line.is_empty())
            {
                new_lines.pop();
            }
            found = seek_sequence(source_lines, &pattern, line_index, chunk.end_of_file);
        }

        let Some(found_index) = found else {
            let failure_kind = previous_context_only_chunk
                .as_ref()
                .and_then(|(prev_index, prev_cursor, prev_pattern)| {
                    if *prev_index + 1 != index || *prev_cursor != line_index {
                        return None;
                    }
                    if prev_pattern == &pattern {
                        return Some(ChunkContextFailureKind::DuplicateAnchorAfterContextOnlyChunk);
                    }
                    None
                })
                .unwrap_or(ChunkContextFailureKind::Generic);
            let failure = ChunkContextFailureDetail {
                kind: failure_kind,
            };
            let (hint_zh, hint_en) =
                build_context_not_found_hint(source_lines, &pattern, line_index, chunk, Some(&failure));
            return Err(patch_error_with_hint(
                "PATCH_CONTEXT_NOT_FOUND",
                format!("补丁应用失败：{path} 第 {} 个变更块找不到匹配上下文", index + 1),
                format!(
                    "Patch apply failed: chunk {} in {} has no matching context",
                    index + 1,
                    path
                ),
                hint_zh,
                hint_en,
            ));
        };

        previous_context_only_chunk = None;
        replacements.push(LineReplacement {
            start: found_index,
            old_len: pattern.len(),
            new_lines,
            display_lines: chunk.lines.clone(),
        });
        line_index = found_index + pattern.len();
    }

    replacements.sort_by_key(|item| item.start);
    Ok(replacements)
}

fn apply_line_replacements(lines: &[String], replacements: &[LineReplacement]) -> Vec<String> {
    let mut result = lines.to_vec();
    for replacement in replacements.iter().rev() {
        for _ in 0..replacement.old_len {
            if replacement.start < result.len() {
                result.remove(replacement.start);
            }
        }
        for (offset, line) in replacement.new_lines.iter().enumerate() {
            result.insert(replacement.start + offset, line.clone());
        }
    }
    result
}

fn normalize_punctuation(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => ' ',
            _ => ch,
        })
        .collect()
}

fn seek_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start.min(lines.len()));
    }
    if pattern.len() > lines.len() {
        return None;
    }

    let max_start = lines.len() - pattern.len();
    let search_start = if eof && lines.len() >= pattern.len() {
        max_start
    } else {
        start
    };
    if search_start > max_start {
        return None;
    }

    let match_at = |index: usize, normalize: &dyn Fn(&str) -> String| -> bool {
        pattern.iter().enumerate().all(|(offset, expected)| {
            normalize(lines[index + offset].as_str()) == normalize(expected.as_str())
        })
    };

    for index in search_start..=max_start {
        if match_at(index, &|value| value.to_string()) {
            return Some(index);
        }
    }
    for index in search_start..=max_start {
        if match_at(index, &|value| value.trim_end().to_string()) {
            return Some(index);
        }
    }
    for index in search_start..=max_start {
        if match_at(index, &|value| value.trim().to_string()) {
            return Some(index);
        }
    }
    for index in search_start..=max_start {
        if match_at(index, &|value| normalize_punctuation(value.trim())) {
            return Some(index);
        }
    }
    None
}

pub(super) fn apply_update_chunks_with_diff(
    source: &str,
    chunks: &[UpdateChunk],
    path: &str,
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<(String, Vec<FileDiffBlock>)> {
    let original_lines = split_lines(source);
    let replacements = compute_chunk_replacements(&original_lines, path, chunks, cancel_probe)?;
    let diff_blocks = build_update_diff_blocks(&replacements);
    let mut new_lines = apply_line_replacements(&original_lines, &replacements);
    if new_lines.is_empty() || new_lines.last().is_some_and(|line| !line.is_empty()) {
        new_lines.push(String::new());
    }
    Ok((new_lines.join("\n"), diff_blocks))
}

#[cfg(test)]
pub(super) fn apply_update_chunks(
    source: &str,
    chunks: &[UpdateChunk],
    path: &str,
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<String> {
    apply_update_chunks_with_diff(source, chunks, path, cancel_probe).map(|(content, _)| content)
}
