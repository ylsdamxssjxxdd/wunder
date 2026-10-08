use super::{
    ensure_patch_not_cancelled_probe, patch_empty_update_line_error, patch_error_with_hint,
    patch_format_error, PatchCancelProbe, ADD_FILE_MARKER, BEGIN_PATCH_MARKER, DELETE_FILE_MARKER,
    END_OF_FILE_MARKER, END_PATCH_MARKER, MOVE_TO_MARKER, PATCH_CANCEL_CHECK_INTERVAL,
    PATCH_INPUT_UNWRAP_MAX_DEPTH, PATCH_MAX_FILE_OPS, PATCH_MAX_UPDATE_CHUNKS,
    PATCH_STRICT_MAX_CHANGED_LINES_PER_CALL, PATCH_STRICT_MAX_UPDATE_CHUNKS_PER_CALL,
    PATCH_STRICT_MAX_UPDATE_CHUNKS_PER_FILE, PATCH_STRICT_MAX_UPDATE_FILES_PER_CALL,
    UPDATE_FILE_MARKER,
};
use anyhow::Result;
use serde_json::Value;

#[derive(Debug, Clone)]
pub(crate) enum ParsedPatchOp {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct UpdateChunk {
    pub(crate) change_context: Option<String>,
    pub(crate) lines: Vec<ChunkLine>,
    pub(crate) end_of_file: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ChunkLine {
    pub(crate) kind: ChunkLineKind,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkLineKind {
    Context,
    Add,
    Delete,
}

pub(super) fn parse_patch_checked(
    input: &str,
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<Vec<ParsedPatchOp>> {
    let parsed_ops = parse_patch(input)?;
    if parsed_ops.is_empty() {
        return Err(patch_error_with_hint(
            "PATCH_FORMAT_EMPTY_PATCH",
            "补丁为空，至少包含一个文件操作",
            "Patch is empty; include at least one file operation",
            "补丁至少应包含一个 Add File / Delete File / Update File 块。",
            "Include at least one Add File / Delete File / Update File block.",
        ));
    }
    if parsed_ops.len() > PATCH_MAX_FILE_OPS {
        return Err(patch_error_with_hint(
            "PATCH_LIMIT_TOO_MANY_FILE_OPS",
            format!("单次补丁文件操作过多（>{PATCH_MAX_FILE_OPS}），请拆分后重试"),
            format!(
                "Patch contains too many file operations (>{PATCH_MAX_FILE_OPS}), split and retry"
            ),
            "请按目录或功能分批提交补丁，降低单次文件操作数。",
            "Submit the patch in batches (by directory or feature) to reduce file operations per call.",
        ));
    }
    let mut total_update_chunks = 0usize;
    for (index, op) in parsed_ops.iter().enumerate() {
        if index % PATCH_CANCEL_CHECK_INTERVAL == 0 {
            if let Some(probe) = cancel_probe {
                ensure_patch_not_cancelled_probe(probe)?;
            }
        }
        if let ParsedPatchOp::Update { chunks, .. } = op {
            total_update_chunks = total_update_chunks.saturating_add(chunks.len());
        }
    }
    if total_update_chunks > PATCH_MAX_UPDATE_CHUNKS {
        return Err(patch_error_with_hint(
            "PATCH_LIMIT_TOO_MANY_CHUNKS",
            format!("补丁变更块过多（>{PATCH_MAX_UPDATE_CHUNKS}），请拆分后重试"),
            format!(
                "Patch contains too many change chunks (>{PATCH_MAX_UPDATE_CHUNKS}), split and retry"
            ),
            "请减少单次 Update File 的块数量，按文件或区域分批提交。",
            "Reduce Update File chunk count per call and submit by file or region.",
        ));
    }
    enforce_patch_edit_scope(&parsed_ops)?;
    Ok(parsed_ops)
}

pub(super) fn enforce_patch_edit_scope(parsed_ops: &[ParsedPatchOp]) -> Result<()> {
    let mut update_files = 0usize;
    let mut update_chunks = 0usize;
    let mut changed_lines = 0usize;
    let mut max_chunks_in_single_file = 0usize;

    for op in parsed_ops {
        if let ParsedPatchOp::Update { chunks, .. } = op {
            update_files = update_files.saturating_add(1);
            update_chunks = update_chunks.saturating_add(chunks.len());
            max_chunks_in_single_file = max_chunks_in_single_file.max(chunks.len());
            changed_lines = changed_lines.saturating_add(
                chunks
                    .iter()
                    .map(|chunk| {
                        chunk
                            .lines
                            .iter()
                            .filter(|line| line.kind != ChunkLineKind::Context)
                            .count()
                    })
                    .sum::<usize>(),
            );
        }
    }

    if update_files <= PATCH_STRICT_MAX_UPDATE_FILES_PER_CALL
        && update_chunks <= PATCH_STRICT_MAX_UPDATE_CHUNKS_PER_CALL
        && max_chunks_in_single_file <= PATCH_STRICT_MAX_UPDATE_CHUNKS_PER_FILE
        && changed_lines <= PATCH_STRICT_MAX_CHANGED_LINES_PER_CALL
    {
        return Ok(());
    }

    Err(patch_error_with_hint(
        "PATCH_SCOPE_TOO_BROAD",
        format!(
            "apply_patch 仍然适合小批量精确编辑；当前补丁包含 {update_files} 个 Update File、{update_chunks} 个变更块、单文件最多 {max_chunks_in_single_file} 个变更块、{changed_lines} 行实际改动，已超过当前允许的放宽范围。"
        ),
        format!(
            "apply_patch is still meant for precise batch edits; this patch has {update_files} Update File blocks, {update_chunks} change chunks, up to {max_chunks_in_single_file} chunks in one file, and {changed_lines} changed lines, which exceeds the current relaxed scope."
        ),
        "请先读取最新文件，再把补丁控制在少量文件和少量区域内；如果单文件有很多分散修改、跨很多文件，或已经非常接近整文件重写，请优先改用 write_file。",
        "Read the latest file first, then keep the patch to a manageable number of files and regions; if one file has many scattered edits, the patch spans many files, or it is already very close to a full rewrite, prefer write_file.",
    ))
}

pub(super) fn extract_patch_input(args: &Value) -> Result<String> {
    for key in ["input", "patch", "content", "raw"] {
        if let Some(value) = args.get(key) {
            if let Some(extracted) = extract_patch_input_candidate(value) {
                return Ok(extracted);
            }
        }
    }
    Err(patch_error_with_hint(
        "PATCH_INPUT_MISSING",
        "缺少补丁内容：请在 input 字段提供完整 patch 文本",
        "Missing patch content; provide the full patch in input",
        "请将完整补丁从 *** Begin Patch 到 *** End Patch 原样放入 input。",
        "Example: pass the full payload from *** Begin Patch to *** End Patch in input.",
    ))
}

pub(super) fn extract_patch_input_candidate(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(unwrapped) = unwrap_nested_patch_input(trimmed) {
                return Some(unwrapped);
            }
            Some(trimmed.to_string())
        }
        Value::Object(_) => {
            let raw = value.to_string();
            unwrap_nested_patch_input(&raw)
        }
        _ => None,
    }
}

pub(super) fn unwrap_nested_patch_input(raw: &str) -> Option<String> {
    let mut current = raw.trim().to_string();
    for _ in 0..PATCH_INPUT_UNWRAP_MAX_DEPTH {
        let trimmed = current.trim();
        if trimmed.is_empty() {
            return None;
        }
        if starts_with_patch_payload(trimmed) {
            return Some(trimmed.to_string());
        }
        let parsed = serde_json::from_str::<Value>(trimmed).ok()?;
        if let Some(next) = extract_nested_patch_value(parsed) {
            current = next;
            continue;
        }
        return None;
    }
    let trimmed = current.trim();
    if starts_with_patch_payload(trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

pub(super) fn extract_nested_patch_value(value: Value) -> Option<String> {
    match value {
        Value::String(inner) => {
            let trimmed = inner.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(map) => {
            for key in ["input", "patch", "content", "raw"] {
                if let Some(next) = map.get(key).and_then(value_to_patch_candidate) {
                    return Some(next);
                }
            }
            None
        }
        _ => None,
    }
}

pub(super) fn value_to_patch_candidate(value: &Value) -> Option<String> {
    match value {
        Value::String(inner) => {
            let trimmed = inner.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(_) => Some(value.to_string()),
        _ => None,
    }
}

pub(super) fn normalize_patch_text(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = repair_patch_envelope(&normalized);
    repair_common_patch_format_issues(&normalized)
}

pub(super) fn repair_patch_envelope(input: &str) -> String {
    let trimmed = strip_surrounding_markdown_fence(input.trim());
    if trimmed.starts_with(BEGIN_PATCH_MARKER) {
        return trimmed.to_string();
    }
    if starts_with_patch_file_op(trimmed)
        && trimmed
            .lines()
            .last()
            .is_some_and(|line| line.trim() == END_PATCH_MARKER)
    {
        return format!("{BEGIN_PATCH_MARKER}\n{trimmed}");
    }
    trimmed.to_string()
}

pub(super) fn strip_surrounding_markdown_fence(input: &str) -> &str {
    let mut lines = input.lines();
    let Some(first) = lines.next() else {
        return input;
    };
    if !first.trim_start().starts_with("```") {
        return input;
    }
    let Some(last) = input.lines().last() else {
        return input;
    };
    if last.trim() != "```" {
        return input;
    }
    let body_start = first.len()
        + input[first.len()..]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
    let body_end = input.len().saturating_sub(last.len());
    input
        .get(body_start..body_end)
        .map(str::trim)
        .unwrap_or(input)
}

pub(super) fn starts_with_patch_payload(input: &str) -> bool {
    input.starts_with(BEGIN_PATCH_MARKER) || starts_with_patch_file_op(input)
}

pub(super) fn starts_with_patch_file_op(input: &str) -> bool {
    input
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| normalized_file_op_header(line))
        .is_some()
}

pub(super) fn repair_common_patch_format_issues(input: &str) -> String {
    let lines = input
        .split('\n')
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    let mut repaired = Vec::with_capacity(lines.len());
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index].clone();
        repaired.push(line.clone());
        index += 1;
        let is_update_header = normalized_file_op_header(&line)
            .is_some_and(|header| header.starts_with(UPDATE_FILE_MARKER));
        if !is_update_header {
            continue;
        }
        if index < lines.len() && lines[index].starts_with(MOVE_TO_MARKER) {
            repaired.push(lines[index].clone());
            index += 1;
        }
        while index < lines.len() && !is_file_op_header(lines[index].as_str()) {
            let raw = lines[index].as_str();
            if raw.trim() == END_OF_FILE_MARKER {
                repaired.push(lines[index].clone());
                index += 1;
                continue;
            }
            if let Some(header) = normalized_update_hunk_header(raw) {
                repaired.push(header);
                index += 1;
                continue;
            }
            let body_start = index;
            while index < lines.len()
                && !is_file_op_header(lines[index].as_str())
                && lines[index].trim() != END_OF_FILE_MARKER
                && normalized_update_hunk_header(lines[index].as_str()).is_none()
            {
                index += 1;
            }
            repaired.extend(repair_update_chunk_lines(&lines[body_start..index]));
        }
    }
    repaired.join("\n")
}

pub(super) fn normalized_update_hunk_header(raw: &str) -> Option<String> {
    if raw == "@@" {
        return Some("@@".to_string());
    }
    if let Some(anchor) = unified_diff_header_anchor(raw) {
        return if anchor.is_empty() {
            Some("@@".to_string())
        } else {
            Some(format!("@@ {anchor}"))
        };
    }
    raw.strip_prefix("@@ ")
        .filter(|anchor| !anchor.trim().is_empty())
        .map(|_| raw.to_string())
}

pub(super) fn unified_diff_header_anchor(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix("@@ ")?;
    let (old_range, rest) = next_token(rest)?;
    let (new_range, rest) = next_token(rest)?;
    let rest = rest.trim_start();
    let anchor = rest.strip_prefix("@@")?.trim();
    old_range
        .strip_prefix('-')
        .filter(|range| is_unified_diff_range(range))?;
    new_range
        .strip_prefix('+')
        .filter(|range| is_unified_diff_range(range))?;
    Some(anchor.to_string())
}

pub(super) fn next_token(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    Some((&trimmed[..end], &trimmed[end..]))
}

pub(super) fn is_unified_diff_range(raw: &str) -> bool {
    let mut parts = raw.split(',');
    let Some(start) = parts.next() else {
        return false;
    };
    if start.is_empty() || !start.chars().all(|ch| ch.is_ascii_digit()) {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => true,
        (Some(count), None) => !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit()),
        _ => false,
    }
}

pub(super) fn repair_update_chunk_lines(lines: &[String]) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }
    let has_separator = lines.iter().any(|line| line.trim() == "***");
    let prefixed_ignoring_separator = lines
        .iter()
        .filter(|line| line.trim() != "***")
        .all(|line| matches!(line.chars().next(), Some(' ') | Some('+') | Some('-')));
    if prefixed_ignoring_separator {
        let repaired = lines
            .iter()
            .filter(|line| line.trim() != "***")
            .map(|line| strip_line_number_from_prefixed_line(line))
            .collect::<Vec<_>>();
        let repaired_numbered_context = lines
            .iter()
            .filter(|line| line.trim() != "***")
            .map(|line| {
                line.starts_with(' ')
                    && strip_display_line_number(line.chars().skip(1).collect::<String>().as_str())
                        .is_some()
            })
            .collect::<Vec<_>>();
        return dedup_repaired_numbered_context_before_delete(
            &repaired,
            &repaired_numbered_context,
        );
    }
    if !has_separator {
        if let Some(repaired) = repair_non_separator_update_chunk_lines(lines) {
            return repaired;
        }
        return lines.to_vec();
    }

    // Repair display-oriented diffs that models often emit after reading numbered file excerpts.
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut in_after = false;
    for line in lines {
        if line.trim() == "***" {
            in_after = true;
            continue;
        }
        let Some(text) = strip_display_line_number(line) else {
            return lines.to_vec();
        };
        if in_after {
            after.push(text);
        } else {
            before.push(text);
        }
    }
    if before.is_empty() || after.is_empty() {
        return lines.to_vec();
    }
    let mut repaired = Vec::with_capacity(before.len() + after.len());
    repaired.extend(before.into_iter().map(|line| format!("-{line}")));
    repaired.extend(after.into_iter().map(|line| format!("+{line}")));
    repaired
}

pub(super) fn repair_non_separator_update_chunk_lines(lines: &[String]) -> Option<Vec<String>> {
    if !lines.iter().any(|line| !line.is_empty()) {
        return None;
    }
    let mut repaired = Vec::with_capacity(lines.len());
    let mut repaired_numbered_context = Vec::with_capacity(lines.len());
    for line in lines {
        if line.is_empty() {
            repaired.push(" ".to_string());
            repaired_numbered_context.push(false);
            continue;
        }
        let Some(prefix) = line.chars().next() else {
            return None;
        };
        match prefix {
            ' ' | '+' | '-' => {
                let body = &line[1..];
                let numbered_context = prefix == ' ' && strip_display_line_number(body).is_some();
                repaired.push(strip_line_number_from_prefixed_line(line));
                repaired_numbered_context.push(numbered_context);
            }
            _ => {
                let content = strip_display_line_number(line)?;
                repaired.push(format!(" {content}"));
                repaired_numbered_context.push(true);
            }
        }
    }
    Some(dedup_repaired_numbered_context_before_delete(
        &repaired,
        &repaired_numbered_context,
    ))
}

// Removing this pattern is only safe when the context line was synthesized from a numbered
// display excerpt. User-authored " context" + "-delete" pairs can be legitimate when the
// source contains repeated adjacent lines.
pub(super) fn dedup_repaired_numbered_context_before_delete(
    lines: &[String],
    repaired_numbered_context: &[bool],
) -> Vec<String> {
    debug_assert_eq!(lines.len(), repaired_numbered_context.len());
    let mut result = Vec::with_capacity(lines.len());
    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
        let prefix = line.chars().next();
        let text: String = line.chars().skip(1).collect();
        if prefix == Some(' ') && repaired_numbered_context[i] && i + 1 < lines.len() {
            let next = &lines[i + 1];
            let next_prefix = next.chars().next();
            let next_text: String = next.chars().skip(1).collect();
            if next_prefix == Some('-') && text == next_text {
                // Skip the context line; keep only the delete line.
                i += 1;
                continue;
            }
        }
        result.push(line.clone());
        i += 1;
    }
    result
}

pub(super) fn strip_line_number_from_prefixed_line(line: &str) -> String {
    let mut chars = line.chars();
    let Some(prefix) = chars.next() else {
        return line.to_string();
    };
    if !matches!(prefix, ' ' | '+' | '-') {
        return line.to_string();
    }
    let body = chars.as_str();
    let stripped = strip_display_line_number(body);
    match stripped {
        Some(content) => format!("{prefix}{content}"),
        None => line.to_string(),
    }
}

pub(super) fn strip_display_line_number(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start();
    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let rest = trimmed.get(digit_count..)?;
    let rest = rest.strip_prefix(':')?;
    let content = rest.strip_prefix(' ').unwrap_or(rest);
    Some(content.to_string())
}

pub(super) fn parse_patch(input: &str) -> Result<Vec<ParsedPatchOp>> {
    let normalized = normalize_patch_text(input);
    let mut lines = normalized
        .split('\n')
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.len() < 3 {
        return Err(patch_format_error(
            "补丁格式错误：至少需要 Begin/操作/End 三行",
            "Invalid patch format: expected at least Begin/operation/End lines",
        ));
    }
    if lines.first().map(|line| line.trim()) != Some(BEGIN_PATCH_MARKER) {
        return Err(patch_format_error(
            format!("补丁格式错误：缺少 {BEGIN_PATCH_MARKER}"),
            format!("Invalid patch format: missing {BEGIN_PATCH_MARKER}"),
        ));
    }
    if lines.last().map(|line| line.trim()) != Some(END_PATCH_MARKER) {
        return Err(patch_format_error(
            format!("补丁格式错误：缺少 {END_PATCH_MARKER}"),
            format!("Invalid patch format: missing {END_PATCH_MARKER}"),
        ));
    }

    let mut ops = Vec::new();
    let mut index = 1usize;
    let end = lines.len().saturating_sub(1);
    while index < end {
        let line = lines[index].as_str();
        let normalized_line = normalized_file_op_header(line).unwrap_or(line);
        if let Some(rest) = normalized_line.strip_prefix(ADD_FILE_MARKER) {
            let path = parse_patch_path(rest, index + 1)?;
            index += 1;
            let mut add_lines = Vec::new();
            while index < end && !is_file_op_header(lines[index].as_str()) {
                let item = lines[index].as_str();
                if normalized_update_hunk_header(item).is_some() {
                    index += 1;
                    continue;
                }
                let content = item.strip_prefix('+').unwrap_or(item);
                add_lines.push(content.to_string());
                index += 1;
            }
            if add_lines.is_empty() {
                return Err(patch_format_error(
                    format!(
                        "补丁格式错误（第 {} 行）：Add File 必须至少包含一行 '+' 内容",
                        index
                    ),
                    format!(
                        "Invalid patch format (line {}): Add File must contain at least one '+' content line",
                        index
                    ),
                ));
            }
            ops.push(ParsedPatchOp::Add {
                path,
                lines: add_lines,
            });
            continue;
        }
        if let Some(rest) = normalized_line.strip_prefix(DELETE_FILE_MARKER) {
            let path = parse_patch_path(rest, index + 1)?;
            ops.push(ParsedPatchOp::Delete { path });
            index += 1;
            continue;
        }
        if let Some(rest) = normalized_line.strip_prefix(UPDATE_FILE_MARKER) {
            let path = parse_patch_path(rest, index + 1)?;
            index += 1;
            let mut move_to = None;
            if index < end {
                if let Some(rest) = lines[index].as_str().strip_prefix(MOVE_TO_MARKER) {
                    move_to = Some(parse_patch_path(rest, index + 1)?);
                    index += 1;
                }
            }
            let mut chunks = Vec::new();
            let mut current = UpdateChunk::default();
            let mut has_change_line = false;
            while index < end && !is_file_op_header(lines[index].as_str()) {
                let raw = lines[index].as_str();
                if raw.trim() == END_OF_FILE_MARKER {
                    current.end_of_file = true;
                    index += 1;
                    continue;
                }
                if raw == "@@" || raw.starts_with("@@ ") {
                    if !current.lines.is_empty() || current.end_of_file {
                        chunks.push(current);
                        current = UpdateChunk::default();
                    }
                    current.change_context = raw
                        .strip_prefix("@@ ")
                        .map(|text| text.to_string())
                        .filter(|text| !text.trim().is_empty());
                    index += 1;
                    continue;
                }
                if raw.is_empty() {
                    // Treat accidental blank separator lines between hunks as non-semantic whitespace.
                    let has_upcoming_chunk_header = lines[index + 1..end]
                        .iter()
                        .find(|line| !line.is_empty())
                        .is_some_and(|line| line.as_str() == "@@" || line.starts_with("@@ "));
                    if has_upcoming_chunk_header {
                        index += 1;
                        continue;
                    }
                    return Err(patch_empty_update_line_error(index + 1));
                }
                let mut chars = raw.chars();
                let Some(marker) = chars.next() else {
                    return Err(patch_empty_update_line_error(index + 1));
                };
                let (kind, text) = match marker {
                    ' ' => (ChunkLineKind::Context, chars.collect::<String>()),
                    '+' => (ChunkLineKind::Add, chars.collect::<String>()),
                    '-' => (ChunkLineKind::Delete, chars.collect::<String>()),
                    _ => {
                        // Models sometimes paste raw source lines into hunks without the required
                        // leading context space. Recover them as context lines instead of failing.
                        (ChunkLineKind::Context, raw.to_string())
                    }
                };
                has_change_line = true;
                current.lines.push(ChunkLine { kind, text });
                index += 1;
            }
            if !current.lines.is_empty() || current.end_of_file {
                chunks.push(current);
            }
            if !has_change_line {
                return Err(patch_format_error(
                    format!("补丁格式错误：Update File {path} 缺少变更内容"),
                    format!("Invalid patch format: Update File {path} has no changes"),
                ));
            }
            ops.push(ParsedPatchOp::Update {
                path,
                move_to,
                chunks,
            });
            continue;
        }
        return Err(patch_format_error(
            format!("补丁格式错误（第 {} 行）：未知文件操作头", index + 1),
            format!(
                "Invalid patch format (line {}): unknown file operation header",
                index + 1
            ),
        ));
    }
    Ok(ops)
}

pub(super) fn parse_patch_path(raw: &str, line_no: usize) -> Result<String> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(patch_format_error(
            format!("补丁格式错误（第 {line_no} 行）：文件路径不能为空"),
            format!("Invalid patch format (line {line_no}): file path cannot be empty"),
        ));
    }
    Ok(path.to_string())
}

pub(super) fn is_file_op_header(line: &str) -> bool {
    normalized_file_op_header(line).is_some() || line.trim() == END_PATCH_MARKER
}

pub(super) fn normalized_file_op_header(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with(ADD_FILE_MARKER)
        || trimmed.starts_with(DELETE_FILE_MARKER)
        || trimmed.starts_with(UPDATE_FILE_MARKER)
    {
        Some(trimmed)
    } else {
        None
    }
}
