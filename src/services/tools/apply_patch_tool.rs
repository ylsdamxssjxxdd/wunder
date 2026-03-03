use super::*;

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const MOVE_TO_MARKER: &str = "*** Move to: ";
const END_OF_FILE_MARKER: &str = "*** End of File";
const PATCH_INPUT_MAX_BYTES: usize = 512 * 1024;
const PATCH_MAX_FILE_OPS: usize = 200;

#[derive(Debug, Clone)]
enum ParsedPatchOp {
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
struct UpdateChunk {
    change_context: Option<String>,
    lines: Vec<ChunkLine>,
    end_of_file: bool,
}

#[derive(Debug, Clone)]
struct ChunkLine {
    kind: ChunkLineKind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkLineKind {
    Context,
    Add,
    Delete,
}

#[derive(Debug, Clone)]
enum ResolvedPatchOp {
    Add {
        path: String,
        target: PathBuf,
        lines: Vec<String>,
    },
    Delete {
        path: String,
        target: PathBuf,
    },
    Update {
        path: String,
        target: PathBuf,
        move_to_path: Option<String>,
        move_to_target: Option<PathBuf>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Debug, Clone)]
struct FileChangeSummary {
    action: String,
    path: String,
    to_path: Option<String>,
    hunks: usize,
}

#[derive(Debug, Clone)]
struct ApplyPatchSummary {
    changed_files: Vec<PathBuf>,
    added: usize,
    updated: usize,
    deleted: usize,
    moved: usize,
    hunks_applied: usize,
    file_summaries: Vec<FileChangeSummary>,
}

#[derive(Debug, Clone)]
enum StagedEntry {
    Existing { content: String },
    Missing,
}

fn is_english_language() -> bool {
    i18n::get_language().to_ascii_lowercase().starts_with("en")
}

fn localized_message(zh: impl Into<String>, en: impl Into<String>) -> String {
    if is_english_language() {
        en.into()
    } else {
        zh.into()
    }
}

fn localized_error(zh: impl Into<String>, en: impl Into<String>) -> anyhow::Error {
    anyhow!("{}", localized_message(zh, en))
}

pub(super) async fn apply_patch(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let input = extract_patch_input(args)?;
    if input.len() > PATCH_INPUT_MAX_BYTES {
        return Err(localized_error(
            format!(
                "补丁过大（超过 {} KB），请拆分后重试",
                PATCH_INPUT_MAX_BYTES / 1024
            ),
            format!(
                "Patch is too large (> {} KB), split it and retry",
                PATCH_INPUT_MAX_BYTES / 1024
            ),
        ));
    }
    let parsed_ops = parse_patch(&input)?;
    if parsed_ops.is_empty() {
        return Err(localized_error(
            "补丁为空，至少包含一个文件操作",
            "Patch is empty; include at least one file operation",
        ));
    }
    if parsed_ops.len() > PATCH_MAX_FILE_OPS {
        return Err(localized_error(
            format!("单次补丁文件操作过多（>{PATCH_MAX_FILE_OPS}），请拆分后重试"),
            format!(
                "Patch contains too many file operations (>{PATCH_MAX_FILE_OPS}), split and retry"
            ),
        ));
    }

    let allow_roots = collect_allow_roots(context);
    let resolved_ops = parsed_ops
        .into_iter()
        .map(|op| resolve_patch_op(context, &allow_roots, op))
        .collect::<Result<Vec<_>>>()?;

    let summary = tokio::task::spawn_blocking(move || apply_patch_ops(resolved_ops))
        .await
        .map_err(|err| anyhow!(err.to_string()))??;

    let workspace_root = context.workspace.workspace_root(context.workspace_id);
    let bump_workspace = summary
        .changed_files
        .iter()
        .any(|path| is_within_root(&workspace_root, path));
    if bump_workspace {
        context.workspace.bump_version(context.workspace_id);
    }

    let mut lsp_records = Vec::new();
    for path in &summary.changed_files {
        if path.exists() {
            let lsp = touch_lsp_file(context, path, true).await;
            lsp_records.push(json!({
                "path": path.to_string_lossy().to_string(),
                "state": lsp,
            }));
        }
    }

    Ok(json!({
        "ok": true,
        "changed_files": summary.changed_files.len(),
        "added": summary.added,
        "updated": summary.updated,
        "deleted": summary.deleted,
        "moved": summary.moved,
        "hunks_applied": summary.hunks_applied,
        "files": summary.file_summaries.into_iter().map(|item| json!({
            "action": item.action,
            "path": item.path,
            "to_path": item.to_path,
            "hunks": item.hunks,
        })).collect::<Vec<_>>(),
        "lsp": lsp_records,
    }))
}

fn extract_patch_input(args: &Value) -> Result<String> {
    for key in ["input", "patch", "content", "raw"] {
        if let Some(value) = args.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    Err(localized_error(
        "缺少补丁内容，请通过 input 传入完整补丁文本",
        "Missing patch content; provide the full patch in input",
    ))
}

fn normalize_patch_text(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

fn parse_patch(input: &str) -> Result<Vec<ParsedPatchOp>> {
    let normalized = normalize_patch_text(input);
    let mut lines = normalized
        .split('\n')
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.len() < 3 {
        return Err(localized_error(
            "补丁格式错误：至少需要 Begin/操作/End 三行",
            "Invalid patch format: expected at least Begin/operation/End lines",
        ));
    }
    if lines.first().map(|line| line.trim()) != Some(BEGIN_PATCH_MARKER) {
        return Err(localized_error(
            format!("补丁格式错误：缺少 {BEGIN_PATCH_MARKER}"),
            format!("Invalid patch format: missing {BEGIN_PATCH_MARKER}"),
        ));
    }
    if lines.last().map(|line| line.trim()) != Some(END_PATCH_MARKER) {
        return Err(localized_error(
            format!("补丁格式错误：缺少 {END_PATCH_MARKER}"),
            format!("Invalid patch format: missing {END_PATCH_MARKER}"),
        ));
    }

    let mut ops = Vec::new();
    let mut index = 1usize;
    let end = lines.len().saturating_sub(1);
    while index < end {
        let line = lines[index].as_str();
        if let Some(rest) = line.strip_prefix(ADD_FILE_MARKER) {
            let path = parse_patch_path(rest, index + 1)?;
            index += 1;
            let mut add_lines = Vec::new();
            while index < end && !is_file_op_header(lines[index].as_str()) {
                let item = lines[index].as_str();
                let Some(content) = item.strip_prefix('+') else {
                    return Err(localized_error(
                        format!("补丁格式错误（第 {} 行）：Add File 仅允许以 '+' 开头", index + 1),
                        format!(
                            "Invalid patch format (line {}): Add File only allows lines prefixed with '+'",
                            index + 1
                        ),
                    ));
                };
                add_lines.push(content.to_string());
                index += 1;
            }
            if add_lines.is_empty() {
                return Err(localized_error(
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
        if let Some(rest) = line.strip_prefix(DELETE_FILE_MARKER) {
            let path = parse_patch_path(rest, index + 1)?;
            ops.push(ParsedPatchOp::Delete { path });
            index += 1;
            continue;
        }
        if let Some(rest) = line.strip_prefix(UPDATE_FILE_MARKER) {
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
                let mut chars = raw.chars();
                let Some(marker) = chars.next() else {
                    return Err(localized_error(
                        format!("补丁格式错误（第 {} 行）：Update File 内容行不能为空", index + 1),
                        format!(
                            "Invalid patch format (line {}): Update File content line cannot be empty",
                            index + 1
                        ),
                    ));
                };
                let text = chars.collect::<String>();
                let kind = match marker {
                    ' ' => ChunkLineKind::Context,
                    '+' => ChunkLineKind::Add,
                    '-' => ChunkLineKind::Delete,
                    _ => {
                        return Err(localized_error(
                            format!(
                                "补丁格式错误（第 {} 行）：Update File 行必须以空格/+/- 开头",
                                index + 1
                            ),
                            format!(
                                "Invalid patch format (line {}): Update File lines must start with space/+/-",
                                index + 1
                            ),
                        ));
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
                return Err(localized_error(
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
        return Err(localized_error(
            format!("补丁格式错误（第 {} 行）：未知文件操作头", index + 1),
            format!("Invalid patch format (line {}): unknown file operation header", index + 1),
        ));
    }
    Ok(ops)
}

fn parse_patch_path(raw: &str, line_no: usize) -> Result<String> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(localized_error(
            format!("补丁格式错误（第 {line_no} 行）：文件路径不能为空"),
            format!("Invalid patch format (line {line_no}): file path cannot be empty"),
        ));
    }
    Ok(path.to_string())
}

fn is_file_op_header(line: &str) -> bool {
    line.starts_with(ADD_FILE_MARKER)
        || line.starts_with(DELETE_FILE_MARKER)
        || line.starts_with(UPDATE_FILE_MARKER)
        || line.trim() == END_PATCH_MARKER
}

fn resolve_patch_op(
    context: &ToolContext<'_>,
    allow_roots: &[PathBuf],
    op: ParsedPatchOp,
) -> Result<ResolvedPatchOp> {
    match op {
        ParsedPatchOp::Add { path, lines } => {
            let target = resolve_tool_path(
                context.workspace.as_ref(),
                context.workspace_id,
                &path,
                allow_roots,
            )?;
            Ok(ResolvedPatchOp::Add {
                path,
                target,
                lines,
            })
        }
        ParsedPatchOp::Delete { path } => {
            let target = resolve_tool_path(
                context.workspace.as_ref(),
                context.workspace_id,
                &path,
                allow_roots,
            )?;
            Ok(ResolvedPatchOp::Delete { path, target })
        }
        ParsedPatchOp::Update {
            path,
            move_to,
            chunks,
        } => {
            let target = resolve_tool_path(
                context.workspace.as_ref(),
                context.workspace_id,
                &path,
                allow_roots,
            )?;
            let (move_to_path, move_to_target) = if let Some(path) = move_to {
                let target_path = resolve_tool_path(
                    context.workspace.as_ref(),
                    context.workspace_id,
                    &path,
                    allow_roots,
                )?;
                (Some(path), Some(target_path))
            } else {
                (None, None)
            };
            Ok(ResolvedPatchOp::Update {
                path,
                target,
                move_to_path,
                move_to_target,
                chunks,
            })
        }
    }
}

fn apply_patch_ops(ops: Vec<ResolvedPatchOp>) -> Result<ApplyPatchSummary> {
    let mut staged: HashMap<PathBuf, StagedEntry> = HashMap::new();
    let mut changed_files = HashSet::new();
    let mut file_summaries = Vec::new();
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut deleted = 0usize;
    let mut moved = 0usize;
    let mut hunks_applied = 0usize;

    for op in ops {
        match op {
            ResolvedPatchOp::Add {
                path,
                target,
                lines,
            } => {
                if target.exists() && target.is_dir() {
                    return Err(localized_error(
                        format!("目标路径是目录，无法写入文件：{path}"),
                        format!("Target path is a directory; cannot write file: {path}"),
                    ));
                }
                if read_staged_or_fs(&target, &staged)?.is_some() {
                    return Err(localized_error(
                        format!("新增失败，文件已存在：{path}"),
                        format!("Add failed: file already exists: {path}"),
                    ));
                }
                let content = join_lines(&lines, true);
                staged.insert(target.clone(), StagedEntry::Existing { content });
                changed_files.insert(target);
                added += 1;
                hunks_applied += 1;
                file_summaries.push(FileChangeSummary {
                    action: "add".to_string(),
                    path,
                    to_path: None,
                    hunks: 1,
                });
            }
            ResolvedPatchOp::Delete { path, target } => {
                let current = read_staged_or_fs(&target, &staged)?;
                if current.is_none() {
                    return Err(localized_error(
                        format!("删除失败，文件不存在：{path}"),
                        format!("Delete failed: file does not exist: {path}"),
                    ));
                }
                if target.exists() && target.is_dir() {
                    return Err(localized_error(
                        format!("删除失败，目标是目录：{path}"),
                        format!("Delete failed: target is a directory: {path}"),
                    ));
                }
                staged.insert(target.clone(), StagedEntry::Missing);
                changed_files.insert(target);
                deleted += 1;
                hunks_applied += 1;
                file_summaries.push(FileChangeSummary {
                    action: "delete".to_string(),
                    path,
                    to_path: None,
                    hunks: 1,
                });
            }
            ResolvedPatchOp::Update {
                path,
                target,
                move_to_path,
                move_to_target,
                chunks,
            } => {
                let Some(source_content) = read_staged_or_fs(&target, &staged)? else {
                    return Err(localized_error(
                        format!("更新失败，文件不存在：{path}"),
                        format!("Update failed: file does not exist: {path}"),
                    ));
                };
                let next_content = apply_update_chunks(&source_content, &chunks, &path)?;
                hunks_applied += chunks.len();
                if let Some(new_target) = move_to_target {
                    if new_target != target && read_staged_or_fs(&new_target, &staged)?.is_some() {
                        let move_to_display = move_to_path
                            .clone()
                            .unwrap_or_else(|| new_target.to_string_lossy().to_string());
                        return Err(localized_error(
                            format!("重命名失败，目标文件已存在：{move_to_display}"),
                            format!(
                                "Rename failed: destination file already exists: {move_to_display}"
                            ),
                        ));
                    }
                    staged.insert(target.clone(), StagedEntry::Missing);
                    staged.insert(
                        new_target.clone(),
                        StagedEntry::Existing {
                            content: next_content,
                        },
                    );
                    changed_files.insert(target);
                    changed_files.insert(new_target);
                    moved += 1;
                } else {
                    staged.insert(
                        target.clone(),
                        StagedEntry::Existing {
                            content: next_content,
                        },
                    );
                    changed_files.insert(target);
                }
                updated += 1;
                file_summaries.push(FileChangeSummary {
                    action: "update".to_string(),
                    path,
                    to_path: move_to_path,
                    hunks: chunks.len(),
                });
            }
        }
    }

    let original_states = snapshot_original_states(&staged)?;
    if let Err(err) = write_staged_entries(&staged) {
        let rollback_error = restore_original_states(&original_states);
        return Err(match rollback_error {
            Ok(()) => localized_error(
                format!("应用补丁失败，已回滚：{err}"),
                format!("Apply patch failed and rollback succeeded: {err}"),
            ),
            Err(restore_err) => localized_error(
                format!("应用补丁失败且回滚异常：{}；原始错误：{}", restore_err, err),
                format!(
                    "Apply patch failed and rollback also failed: {restore_err}; original error: {err}"
                ),
            ),
        });
    }

    Ok(ApplyPatchSummary {
        changed_files: changed_files.into_iter().collect(),
        added,
        updated,
        deleted,
        moved,
        hunks_applied,
        file_summaries,
    })
}

fn snapshot_original_states(
    staged: &HashMap<PathBuf, StagedEntry>,
) -> Result<HashMap<PathBuf, Option<Vec<u8>>>> {
    let mut states = HashMap::with_capacity(staged.len());
    for path in staged.keys() {
        let state = if path.exists() {
            if path.is_dir() {
                return Err(localized_error(
                    format!("路径是目录，无法执行补丁写入：{}", path.display()),
                    format!(
                        "Path is a directory; cannot apply staged patch write: {}",
                        path.display()
                    ),
                ));
            }
            Some(fs::read(path)?)
        } else {
            None
        };
        states.insert(path.clone(), state);
    }
    Ok(states)
}

fn write_staged_entries(staged: &HashMap<PathBuf, StagedEntry>) -> Result<()> {
    for (path, entry) in staged {
        match entry {
            StagedEntry::Existing { content } => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, content)?;
            }
            StagedEntry::Missing => {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}

fn restore_original_states(original_states: &HashMap<PathBuf, Option<Vec<u8>>>) -> Result<()> {
    for (path, state) in original_states {
        match state {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, bytes)?;
            }
            None => {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}

fn read_staged_or_fs(
    path: &Path,
    staged: &HashMap<PathBuf, StagedEntry>,
) -> Result<Option<String>> {
    if let Some(entry) = staged.get(path) {
        return Ok(match entry {
            StagedEntry::Existing { content } => Some(content.clone()),
            StagedEntry::Missing => None,
        });
    }
    if !path.exists() {
        return Ok(None);
    }
    if path.is_dir() {
        return Err(localized_error(
            format!("路径是目录，不支持文本补丁：{}", path.display()),
            format!(
                "Path is a directory; text patch is not supported: {}",
                path.display()
            ),
        ));
    }
    let content = fs::read_to_string(path)?;
    Ok(Some(content))
}

fn split_lines(content: &str) -> Vec<String> {
    let normalized = normalize_patch_text(content);
    let mut lines = normalized
        .split('\n')
        .map(|line| line.to_string())
        .collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn join_lines(lines: &[String], ensure_newline: bool) -> String {
    let mut text = lines.join("\n");
    if ensure_newline && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn apply_update_chunks(source: &str, chunks: &[UpdateChunk], path: &str) -> Result<String> {
    let mut lines = split_lines(source);
    let mut cursor = 0usize;
    for (index, chunk) in chunks.iter().enumerate() {
        let old_lines = chunk
            .lines
            .iter()
            .filter(|line| line.kind != ChunkLineKind::Add)
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        let new_lines = chunk
            .lines
            .iter()
            .filter(|line| line.kind != ChunkLineKind::Delete)
            .map(|line| line.text.clone())
            .collect::<Vec<_>>();
        let (start, end) =
            find_chunk_range(&lines, cursor, chunk, &old_lines).ok_or_else(|| {
                localized_error(
                    format!("补丁应用失败：{} 第 {} 个变更块找不到匹配上下文", path, index + 1),
                    format!(
                        "Patch apply failed: chunk {} in {} has no matching context",
                        index + 1,
                        path
                    ),
                )
            })?;
        lines.splice(start..end, new_lines.iter().cloned());
        cursor = start + new_lines.len();
    }
    Ok(join_lines(&lines, true))
}

fn find_chunk_range(
    source_lines: &[String],
    cursor: usize,
    chunk: &UpdateChunk,
    old_lines: &[String],
) -> Option<(usize, usize)> {
    let len = source_lines.len();
    let mut search_start = cursor.min(len);
    if let Some(anchor) = chunk
        .change_context
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        if let Some(anchor_index) = source_lines
            .iter()
            .enumerate()
            .skip(search_start)
            .find_map(|(idx, line)| line.contains(anchor).then_some(idx))
        {
            search_start = anchor_index;
        }
    }

    if old_lines.is_empty() {
        let start = if chunk.end_of_file { len } else { search_start };
        return Some((start, start));
    }
    if old_lines.len() > len {
        return None;
    }
    for start in search_start..=len.saturating_sub(old_lines.len()) {
        let end = start + old_lines.len();
        if chunk.end_of_file && end != len {
            continue;
        }
        if source_lines[start..end] == *old_lines {
            return Some((start, end));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("wunder-{prefix}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn parse_patch_supports_add_delete_update() {
        let patch = r#"*** Begin Patch
*** Add File: demo.txt
+hello
*** Update File: demo.txt
@@
-hello
+world
*** Delete File: old.txt
*** End Patch"#;
        let ops = parse_patch(patch).expect("patch should parse");
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn apply_update_chunks_replaces_content() {
        let chunk = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "b".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "x".to_string(),
                },
            ],
            end_of_file: false,
        };
        let output =
            apply_update_chunks("a\nb\nc\n", &[chunk], "demo.txt").expect("chunk should apply");
        assert_eq!(output, "a\nx\nc\n");
    }

    #[test]
    fn apply_patch_ops_rejects_add_when_file_exists() {
        let dir = create_temp_dir("apply-patch-add-exists");
        let existing = dir.join("existing.txt");
        fs::write(&existing, "old\n").expect("seed file should be written");

        let result = apply_patch_ops(vec![ResolvedPatchOp::Add {
            path: "existing.txt".to_string(),
            target: existing,
            lines: vec!["new".to_string()],
        }]);

        assert!(result.is_err());
        let message = result.expect_err("should reject overwrite").to_string();
        assert!(message.contains("文件已存在") || message.contains("already exists"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_patch_ops_rejects_move_when_destination_exists() {
        let dir = create_temp_dir("apply-patch-move-exists");
        let source = dir.join("source.txt");
        let destination = dir.join("destination.txt");
        fs::write(&source, "alpha\n").expect("source should be written");
        fs::write(&destination, "occupied\n").expect("destination should be written");

        let chunk = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "alpha".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "beta".to_string(),
                },
            ],
            end_of_file: false,
        };
        let result = apply_patch_ops(vec![ResolvedPatchOp::Update {
            path: "source.txt".to_string(),
            target: source,
            move_to_path: Some("destination.txt".to_string()),
            move_to_target: Some(destination),
            chunks: vec![chunk],
        }]);

        assert!(result.is_err());
        let message = result
            .expect_err("should reject occupied move target")
            .to_string();
        assert!(message.contains("目标文件已存在") || message.contains("already exists"));

        let _ = fs::remove_dir_all(&dir);
    }
}
