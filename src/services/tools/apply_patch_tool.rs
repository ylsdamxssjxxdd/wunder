use super::apply_patch_update::apply_update_chunks_with_diff as apply_update_chunks_with_diff_engine;
use super::apply_patch_update::{ChunkContextFailureDetail, ChunkContextFailureKind};
use super::command_options::parse_dry_run;
use super::*;
use crate::core::atomic_write::{atomic_write_bytes, atomic_write_text};
use crate::monitor::MonitorState;

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const MOVE_TO_MARKER: &str = "*** Move to: ";
const END_OF_FILE_MARKER: &str = "*** End of File";
const PATCH_INPUT_MAX_BYTES: usize = 512 * 1024;
const PATCH_MAX_FILE_OPS: usize = 200;
const PATCH_MAX_UPDATE_CHUNKS: usize = 1000;
const PATCH_STRICT_MAX_UPDATE_FILES_PER_CALL: usize = 4;
const PATCH_STRICT_MAX_UPDATE_CHUNKS_PER_CALL: usize = 12;
const PATCH_STRICT_MAX_UPDATE_CHUNKS_PER_FILE: usize = 8;
const PATCH_STRICT_MAX_CHANGED_LINES_PER_CALL: usize = 480;
const PATCH_INPUT_UNWRAP_MAX_DEPTH: usize = 8;
pub(super) const PATCH_CANCEL_CHECK_INTERVAL: usize = 32;

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
pub(super) struct UpdateChunk {
    pub(super) change_context: Option<String>,
    pub(super) lines: Vec<ChunkLine>,
    pub(super) end_of_file: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ChunkLine {
    pub(super) kind: ChunkLineKind,
    pub(super) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChunkLineKind {
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
    diff_blocks: Vec<FileDiffBlock>,
}

#[derive(Debug, Clone, Copy, Default)]
struct UpdateChunkEffect {
    has_context: bool,
    context_lines: usize,
    has_add: bool,
    has_delete: bool,
    old_len: usize,
    new_len: usize,
    looks_like_missing_prefixes: bool,
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
    no_effect_updates: Vec<String>,
    no_effect_chunk_effects: Vec<Vec<UpdateChunkEffect>>,
}

#[derive(Debug, Clone)]
pub(super) struct FileDiffBlock {
    pub(super) header: String,
    pub(super) start_line_before: usize,
    pub(super) end_line_before: usize,
    pub(super) start_line_after: usize,
    pub(super) end_line_after: usize,
    pub(super) lines: Vec<FileDiffLine>,
}

#[derive(Debug, Clone)]
pub(super) struct FileDiffLine {
    pub(super) kind: &'static str,
    pub(super) old_line: Option<usize>,
    pub(super) new_line: Option<usize>,
    pub(super) text: String,
}

#[derive(Debug, Clone)]
enum StagedEntry {
    Existing { content: String },
    Missing,
}

#[derive(Debug, Clone)]
struct PatchToolError {
    code: &'static str,
    message: String,
    hint: Option<String>,
    retryable: bool,
}

#[derive(Clone)]
pub(super) struct PatchCancelProbe {
    pub(super) monitor: Arc<MonitorState>,
    pub(super) session_id: String,
}

impl PatchToolError {
    fn new(code: &'static str, message: String, hint: Option<String>, retryable: bool) -> Self {
        Self {
            code,
            message,
            hint,
            retryable,
        }
    }
}

impl std::fmt::Display for PatchToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for PatchToolError {}

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

pub(super) fn patch_error_with_hint(
    code: &'static str,
    zh: impl Into<String>,
    en: impl Into<String>,
    hint_zh: impl Into<String>,
    hint_en: impl Into<String>,
) -> anyhow::Error {
    let message = localized_message(zh, en);
    let hint = localized_message(hint_zh, hint_en);
    anyhow::Error::new(PatchToolError::new(code, message, Some(hint), true))
}

fn patch_format_error(zh: impl Into<String>, en: impl Into<String>) -> anyhow::Error {
    patch_error_with_hint(
        "PATCH_FORMAT_INVALID",
        zh,
        en,
        "请检查 Begin/End 标记、操作头（Add/Delete/Update）和行前缀（空格/+/-）。",
        "Check Begin/End markers, operation headers (Add/Delete/Update), and line prefixes (space/+/-).",
    )
}

fn patch_empty_update_line_error(line_no: usize) -> anyhow::Error {
    patch_error_with_hint(
        "PATCH_FORMAT_INVALID",
        format!("补丁格式错误（第 {line_no} 行）：Update File 内容行不能为空"),
        format!("Invalid patch format (line {line_no}): Update File content line cannot be empty"),
        "在 @@ 之后，空白行也必须带前缀。请把空白行写成单个空格开头的一行（\" \"），不要直接留空。",
        "After @@, blank lines also require a prefix. Represent a blank context line as a single leading space (\" \"), not an empty line.",
    )
}

fn build_patch_error_result(error: anyhow::Error) -> Value {
    if let Some(detail) = error.downcast_ref::<PatchToolError>() {
        let mut data = serde_json::Map::new();
        data.insert(
            "error_code".to_string(),
            Value::String(detail.code.to_string()),
        );
        data.insert("retryable".to_string(), Value::Bool(detail.retryable));
        if let Some(hint) = detail.hint.as_ref().filter(|text| !text.trim().is_empty()) {
            data.insert("hint".to_string(), Value::String(hint.clone()));
        }
        return json!({
            "ok": false,
            "error": detail.message,
            "error_meta": {
                "code": detail.code,
                "hint": detail.hint.clone(),
                "retryable": detail.retryable,
                "retry_after_ms": Value::Null,
            },
            "data": Value::Object(data),
        });
    }

    let message = localized_message(
        "应用补丁失败：发生未分类错误",
        "Apply patch failed: unclassified error",
    );
    let hint = localized_message(
        "请缩小补丁范围后重试；若持续失败，请记录补丁与日志供排查。",
        "Retry with a smaller patch; if it keeps failing, capture the patch and logs for diagnosis.",
    );
    let hint_for_meta = hint.clone();
    json!({
        "ok": false,
        "error": message,
        "error_meta": {
            "code": "PATCH_UNKNOWN",
            "hint": hint_for_meta,
            "retryable": false,
            "retry_after_ms": Value::Null,
        },
        "data": {
            "error_code": "PATCH_UNKNOWN",
            "retryable": false,
            "hint": hint,
            "detail": error.to_string(),
        }
    })
}

pub(super) async fn apply_patch(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if let Some(result) = super::execute_in_sandbox(context, "应用补丁", args).await {
        if !parse_dry_run(args) {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }
    match apply_patch_inner(context, args).await {
        Ok(value) => Ok(value),
        Err(error) => Ok(build_patch_error_result(error)),
    }
}

async fn apply_patch_inner(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let dry_run = parse_dry_run(args);
    let input = extract_patch_input(args)?;
    if input.len() > PATCH_INPUT_MAX_BYTES {
        return Err(patch_error_with_hint(
            "PATCH_LIMIT_INPUT_TOO_LARGE",
            format!(
                "补丁过大（超过 {} KB），请拆分后重试",
                PATCH_INPUT_MAX_BYTES / 1024
            ),
            format!(
                "Patch is too large (> {} KB), split it and retry",
                PATCH_INPUT_MAX_BYTES / 1024
            ),
            "将补丁拆成多个更小批次（每批只改少量文件）再提交。",
            "Split the patch into smaller batches (each touching fewer files).",
        ));
    }
    ensure_patch_not_cancelled(context)?;
    let parsed_ops = tokio::task::spawn_blocking({
        let input = input.clone();
        let cancel_probe = build_patch_cancel_probe(context);
        move || parse_patch_checked(&input, cancel_probe.as_ref())
    })
    .await
    .map_err(|err| {
        patch_error_with_hint(
            "PATCH_RUNTIME_TASK_FAILED",
            format!("应用补丁预处理任务执行失败：{err}"),
            format!("Apply patch preprocessing task failed: {err}"),
            "请重试；若持续失败请检查运行时环境是否稳定。",
            "Retry; if this persists, verify runtime stability.",
        )
    })??;

    let allow_roots = collect_allow_roots(context);
    let resolved_ops = parsed_ops
        .into_iter()
        .map(|op| resolve_patch_op(context, &allow_roots, op))
        .collect::<Result<Vec<_>>>()?;

    if dry_run {
        let summary = summarize_patch_ops(&resolved_ops);
        return Ok(build_model_tool_success(
            "apply_patch",
            "dry_run",
            format!(
                "Validated patch touching {} files without applying it.",
                summary.changed_files.len()
            ),
            json!({
                "dry_run": true,
                "changed_files": summary.changed_files.len(),
                "added": summary.added,
                "updated": summary.updated,
                "deleted": summary.deleted,
                "moved": summary.moved,
                "hunks_applied": summary.hunks_applied,
                "no_effect_updates": summary.no_effect_updates,
                "files": summary.file_summaries.into_iter().map(|item| json!({
                    "action": item.action,
                    "path": item.path,
                    "to_path": item.to_path,
                    "hunks": item.hunks,
                    "diff_blocks": item.diff_blocks.iter().map(|block| json!({
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
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
                "lsp": Vec::<Value>::new(),
            }),
        ));
    }

    ensure_patch_not_cancelled(context)?;
    let summary = tokio::task::spawn_blocking({
        let cancel_probe = build_patch_cancel_probe(context);
        move || apply_patch_ops_checked(resolved_ops, cancel_probe.as_ref())
    })
    .await
    .map_err(|err| {
        patch_error_with_hint(
            "PATCH_RUNTIME_TASK_FAILED",
            format!("应用补丁任务执行失败：{err}"),
            format!("Apply patch worker task failed: {err}"),
            "请重试；若持续失败请检查运行时环境是否稳定。",
            "Retry; if this persists, verify runtime stability.",
        )
    })??;

    if !summary.no_effect_updates.is_empty()
        && summary.added == 0
        && summary.deleted == 0
        && summary.moved == 0
        && summary.changed_files.is_empty()
    {
        let sample = summary
            .no_effect_updates
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let (zh_hint, en_hint) = build_patch_no_effect_hint(&summary.no_effect_chunk_effects);
        return Err(patch_error_with_hint(
            "PATCH_NO_EFFECT",
            format!("补丁没有产生任何实际修改：{sample}"),
            format!("Patch produced no effective change: {sample}"),
            zh_hint,
            en_hint,
        ));
    }

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

    Ok(build_model_tool_success(
        "apply_patch",
        "completed",
        format!(
            "Applied patch touching {} files.",
            summary.changed_files.len()
        ),
        json!({
            "changed_files": summary.changed_files.len(),
            "added": summary.added,
            "updated": summary.updated,
            "deleted": summary.deleted,
            "moved": summary.moved,
            "hunks_applied": summary.hunks_applied,
            "no_effect_updates": summary.no_effect_updates,
            "files": summary.file_summaries.into_iter().map(|item| json!({
                "action": item.action,
                "path": item.path,
                "to_path": item.to_path,
                "hunks": item.hunks,
                "diff_blocks": item.diff_blocks.iter().map(|block| json!({
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
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "lsp": lsp_records,
        }),
    ))
}

fn summarize_patch_ops(ops: &[ResolvedPatchOp]) -> ApplyPatchSummary {
    let mut changed_files = HashSet::new();
    let mut file_summaries = Vec::new();
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut deleted = 0usize;
    let mut moved = 0usize;
    let mut hunks_applied = 0usize;

    for op in ops {
        match op {
            ResolvedPatchOp::Add { path, target, .. } => {
                added += 1;
                changed_files.insert(target.clone());
                file_summaries.push(FileChangeSummary {
                    action: "add".to_string(),
                    path: path.clone(),
                    to_path: None,
                    hunks: 0,
                    diff_blocks: Vec::new(),
                });
            }
            ResolvedPatchOp::Delete { path, target } => {
                deleted += 1;
                changed_files.insert(target.clone());
                file_summaries.push(FileChangeSummary {
                    action: "delete".to_string(),
                    path: path.clone(),
                    to_path: None,
                    hunks: 0,
                    diff_blocks: Vec::new(),
                });
            }
            ResolvedPatchOp::Update {
                path,
                target,
                move_to_path,
                move_to_target,
                chunks,
            } => {
                updated += 1;
                let hunks = chunks.len();
                hunks_applied += hunks;
                changed_files.insert(target.clone());
                if let Some(new_target) = move_to_target.as_ref() {
                    moved += 1;
                    changed_files.insert(new_target.clone());
                }
                file_summaries.push(FileChangeSummary {
                    action: "update".to_string(),
                    path: path.clone(),
                    to_path: move_to_path.clone(),
                    hunks,
                    diff_blocks: Vec::new(),
                });
            }
        }
    }

    ApplyPatchSummary {
        changed_files: changed_files.into_iter().collect(),
        added,
        updated,
        deleted,
        moved,
        hunks_applied,
        file_summaries,
        no_effect_updates: Vec::new(),
        no_effect_chunk_effects: Vec::new(),
    }
}

fn build_patch_cancel_probe(context: &ToolContext<'_>) -> Option<PatchCancelProbe> {
    let monitor = context.monitor.as_ref()?.clone();
    let session_id = context.session_id.trim();
    if session_id.is_empty() {
        return None;
    }
    Some(PatchCancelProbe {
        monitor,
        session_id: session_id.to_string(),
    })
}

fn ensure_patch_not_cancelled(context: &ToolContext<'_>) -> Result<()> {
    if let Some(probe) = build_patch_cancel_probe(context) {
        ensure_patch_not_cancelled_probe(&probe)?;
    }
    Ok(())
}

pub(super) fn ensure_patch_not_cancelled_probe(probe: &PatchCancelProbe) -> Result<()> {
    if probe.monitor.is_cancelled(&probe.session_id) {
        return Err(patch_cancelled_error());
    }
    Ok(())
}

fn patch_cancelled_error() -> anyhow::Error {
    patch_error_with_hint(
        "CANCELLED",
        "补丁已中止，因为会话已请求停止",
        "Patch cancelled because the session received a stop request",
        "如需继续，请在确认最新文件状态后重新发起编辑或恢复流程。",
        "If you still need the edit, re-read the latest file state and start a new edit or resume flow.",
    )
}

fn parse_patch_checked(
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

fn enforce_patch_edit_scope(parsed_ops: &[ParsedPatchOp]) -> Result<()> {
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

fn extract_patch_input(args: &Value) -> Result<String> {
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

fn extract_patch_input_candidate(value: &Value) -> Option<String> {
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

fn unwrap_nested_patch_input(raw: &str) -> Option<String> {
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

fn extract_nested_patch_value(value: Value) -> Option<String> {
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

fn value_to_patch_candidate(value: &Value) -> Option<String> {
    match value {
        Value::String(inner) => {
            let trimmed = inner.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Object(_) => Some(value.to_string()),
        _ => None,
    }
}

fn normalize_patch_text(input: &str) -> String {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = repair_patch_envelope(&normalized);
    repair_common_patch_format_issues(&normalized)
}

fn repair_patch_envelope(input: &str) -> String {
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

fn strip_surrounding_markdown_fence(input: &str) -> &str {
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

fn starts_with_patch_payload(input: &str) -> bool {
    input.starts_with(BEGIN_PATCH_MARKER) || starts_with_patch_file_op(input)
}

fn starts_with_patch_file_op(input: &str) -> bool {
    input
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| normalized_file_op_header(line))
        .is_some()
}

fn repair_common_patch_format_issues(input: &str) -> String {
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

fn normalized_update_hunk_header(raw: &str) -> Option<String> {
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

fn unified_diff_header_anchor(raw: &str) -> Option<String> {
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

fn next_token(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    Some((&trimmed[..end], &trimmed[end..]))
}

fn is_unified_diff_range(raw: &str) -> bool {
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

fn repair_update_chunk_lines(lines: &[String]) -> Vec<String> {
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

fn repair_non_separator_update_chunk_lines(lines: &[String]) -> Option<Vec<String>> {
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
fn dedup_repaired_numbered_context_before_delete(
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

fn strip_line_number_from_prefixed_line(line: &str) -> String {
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

fn strip_display_line_number(raw: &str) -> Option<String> {
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

fn parse_patch_path(raw: &str, line_no: usize) -> Result<String> {
    let path = raw.trim();
    if path.is_empty() {
        return Err(patch_format_error(
            format!("补丁格式错误（第 {line_no} 行）：文件路径不能为空"),
            format!("Invalid patch format (line {line_no}): file path cannot be empty"),
        ));
    }
    Ok(path.to_string())
}

fn is_file_op_header(line: &str) -> bool {
    normalized_file_op_header(line).is_some() || line.trim() == END_PATCH_MARKER
}

fn normalized_file_op_header(line: &str) -> Option<&str> {
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

fn apply_patch_ops_checked(
    ops: Vec<ResolvedPatchOp>,
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<ApplyPatchSummary> {
    if let Some(probe) = cancel_probe {
        ensure_patch_not_cancelled_probe(probe)?;
    }
    apply_patch_ops(ops, cancel_probe)
}

fn apply_patch_ops(
    ops: Vec<ResolvedPatchOp>,
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<ApplyPatchSummary> {
    let mut staged: HashMap<PathBuf, StagedEntry> = HashMap::new();
    let mut changed_files = HashSet::new();
    let mut file_summaries = Vec::new();
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut deleted = 0usize;
    let mut moved = 0usize;
    let mut hunks_applied = 0usize;
    let mut no_effect_updates = Vec::new();
    let mut no_effect_chunk_effects = Vec::new();

    for (op_index, op) in ops.into_iter().enumerate() {
        if op_index % PATCH_CANCEL_CHECK_INTERVAL == 0 {
            if let Some(probe) = cancel_probe {
                ensure_patch_not_cancelled_probe(probe)?;
            }
        }
        match op {
            ResolvedPatchOp::Add {
                path,
                target,
                lines,
            } => {
                if target.exists() && target.is_dir() {
                    return Err(patch_error_with_hint(
                        "PATCH_TARGET_IS_DIRECTORY",
                        format!("目标路径是目录，无法写入文件：{path}"),
                        format!("Target path is a directory; cannot write file: {path}"),
                        "请确认 Add File 的目标是文件路径，而不是目录路径。",
                        "Ensure Add File points to a file path, not a directory path.",
                    ));
                }
                if read_staged_or_fs(&target, &staged)?.is_some() {
                    return Err(patch_error_with_hint(
                        "PATCH_CONFLICT_FILE_EXISTS",
                        format!("新增失败，文件已存在：{path}"),
                        format!("Add failed: file already exists: {path}"),
                        "若需修改现有文件，请改用 Update File；若需覆盖请先 Delete File。",
                        "Use Update File to modify existing files, or Delete File first before replacing.",
                    ));
                }
                let content = join_lines(&lines, true);
                staged.insert(target.clone(), StagedEntry::Existing { content });
                changed_files.insert(target);
                added += 1;
                hunks_applied += 1;
                file_summaries.push(FileChangeSummary {
                    action: "add".to_string(),
                    path: path.clone(),
                    to_path: None,
                    hunks: 1,
                    diff_blocks: vec![build_add_file_diff_block(&lines)],
                });
            }
            ResolvedPatchOp::Delete { path, target } => {
                let current = read_staged_or_fs(&target, &staged)?;
                if current.is_none() {
                    return Err(patch_error_with_hint(
                        "PATCH_CONFLICT_FILE_NOT_FOUND",
                        format!("删除失败，文件不存在：{path}"),
                        format!("Delete failed: file does not exist: {path}"),
                        "确认路径是否正确，或先读取目录/文件再执行删除。",
                        "Verify the path first, or read the directory/file before deleting.",
                    ));
                }
                if target.exists() && target.is_dir() {
                    return Err(patch_error_with_hint(
                        "PATCH_TARGET_IS_DIRECTORY",
                        format!("删除失败，目标是目录：{path}"),
                        format!("Delete failed: target is a directory: {path}"),
                        "当前补丁仅支持文件删除；目录请改用命令工具处理。",
                        "The patch tool only supports file deletion; use command tools for directories.",
                    ));
                }
                staged.insert(target.clone(), StagedEntry::Missing);
                changed_files.insert(target);
                deleted += 1;
                hunks_applied += 1;
                file_summaries.push(FileChangeSummary {
                    action: "delete".to_string(),
                    path: path.clone(),
                    to_path: None,
                    hunks: 1,
                    diff_blocks: vec![build_delete_file_diff_block(
                        current.as_deref().unwrap_or_default(),
                    )],
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
                    return Err(patch_error_with_hint(
                        "PATCH_CONFLICT_FILE_NOT_FOUND",
                        format!("更新失败，文件不存在：{path}"),
                        format!("Update failed: file does not exist: {path}"),
                        "请确认 Update File 路径存在，或先用 Add File 创建该文件。",
                        "Ensure the Update File path exists, or create it first via Add File.",
                    ));
                };
                let (next_content, diff_blocks) = apply_update_chunks_with_diff_engine(
                    &source_content,
                    &chunks,
                    &path,
                    cancel_probe,
                )?;
                let had_effect = next_content != source_content
                    || move_to_target
                        .as_ref()
                        .is_some_and(|new_target| *new_target != target);
                if !had_effect {
                    no_effect_updates.push(path.clone());
                    no_effect_chunk_effects.push(
                        chunks
                            .iter()
                            .map(analyze_update_chunk_effect)
                            .collect::<Vec<_>>(),
                    );
                    continue;
                }
                hunks_applied += chunks.len();
                if let Some(new_target) = move_to_target {
                    if new_target != target && read_staged_or_fs(&new_target, &staged)?.is_some() {
                        let move_to_display = move_to_path
                            .clone()
                            .unwrap_or_else(|| new_target.to_string_lossy().to_string());
                        return Err(patch_error_with_hint(
                            "PATCH_CONFLICT_FILE_EXISTS",
                            format!("重命名失败，目标文件已存在：{move_to_display}"),
                            format!(
                                "Rename failed: destination file already exists: {move_to_display}"
                            ),
                            "请更换 Move to 目标路径，或先删除同名目标文件。",
                            "Choose a different Move to destination, or delete the existing target file first.",
                        ));
                    }
                    staged.insert(target.clone(), StagedEntry::Missing);
                    staged.insert(
                        new_target.clone(),
                        StagedEntry::Existing {
                            content: next_content.clone(),
                        },
                    );
                    changed_files.insert(target);
                    changed_files.insert(new_target);
                    moved += 1;
                } else {
                    staged.insert(
                        target.clone(),
                        StagedEntry::Existing {
                            content: next_content.clone(),
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
                    diff_blocks,
                });
            }
        }
    }

    let original_states = snapshot_original_states(&staged)?;
    if let Err(err) = write_staged_entries(&staged) {
        let rollback_error = restore_original_states(&original_states);
        return Err(match rollback_error {
            Ok(()) => patch_error_with_hint(
                "PATCH_IO_WRITE_FAILED",
                format!("应用补丁失败，已回滚：{err}"),
                format!("Apply patch failed and rollback succeeded: {err}"),
                "请检查文件权限、磁盘空间与路径可写性后重试。",
                "Check file permissions, disk space, and path writability before retrying.",
            ),
            Err(restore_err) => patch_error_with_hint(
                "PATCH_IO_ROLLBACK_FAILED",
                format!("应用补丁失败且回滚异常：{}；原始错误：{}", restore_err, err),
                format!(
                    "Apply patch failed and rollback also failed: {restore_err}; original error: {err}"
                ),
                "检测到写入与回滚都失败，请立即人工检查受影响文件状态。",
                "Both write and rollback failed; immediately inspect affected files manually.",
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
        no_effect_updates,
        no_effect_chunk_effects,
    })
}

fn snapshot_original_states(
    staged: &HashMap<PathBuf, StagedEntry>,
) -> Result<HashMap<PathBuf, Option<Vec<u8>>>> {
    let mut states = HashMap::with_capacity(staged.len());
    for path in staged.keys() {
        let state = if path.exists() {
            if path.is_dir() {
                return Err(patch_error_with_hint(
                    "PATCH_TARGET_IS_DIRECTORY",
                    format!("路径是目录，无法执行补丁写入：{}", path.display()),
                    format!(
                        "Path is a directory; cannot apply staged patch write: {}",
                        path.display()
                    ),
                    "请确认补丁仅操作文件路径。",
                    "Ensure the patch only targets file paths.",
                ));
            }
            Some(fs::read(path).map_err(|err| {
                patch_error_with_hint(
                    "PATCH_IO_READ_FAILED",
                    format!("读取原始文件内容失败：{} ({err})", path.display()),
                    format!(
                        "Failed to read original file content: {} ({err})",
                        path.display()
                    ),
                    "请检查文件权限与可访问性后重试。",
                    "Check file permissions and accessibility, then retry.",
                )
            })?)
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
                    fs::create_dir_all(parent).map_err(|err| {
                        patch_error_with_hint(
                            "PATCH_IO_WRITE_FAILED",
                            format!("创建父目录失败：{} ({err})", parent.display()),
                            format!(
                                "Failed to create parent directory: {} ({err})",
                                parent.display()
                            ),
                            "请检查目录权限与磁盘空间后重试。",
                            "Check directory permissions and disk space, then retry.",
                        )
                    })?;
                }
                atomic_write_text(path, content).map_err(|err| {
                    patch_error_with_hint(
                        "PATCH_IO_WRITE_FAILED",
                        format!("写入文件失败：{} ({err})", path.display()),
                        format!("Failed to write file: {} ({err})", path.display()),
                        "请检查文件权限、磁盘空间与路径可写性后重试。",
                        "Check file permissions, disk space, and path writability before retrying.",
                    )
                })?;
            }
            StagedEntry::Missing => {
                if path.exists() {
                    fs::remove_file(path).map_err(|err| {
                        patch_error_with_hint(
                            "PATCH_IO_WRITE_FAILED",
                            format!("删除文件失败：{} ({err})", path.display()),
                            format!("Failed to remove file: {} ({err})", path.display()),
                            "请确认文件未被占用且当前进程有删除权限。",
                            "Ensure the file is not locked and the process has delete permission.",
                        )
                    })?;
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
                    fs::create_dir_all(parent).map_err(|err| {
                        patch_error_with_hint(
                            "PATCH_IO_ROLLBACK_FAILED",
                            format!("回滚时创建父目录失败：{} ({err})", parent.display()),
                            format!(
                                "Rollback failed to create parent directory: {} ({err})",
                                parent.display()
                            ),
                            "请检查目录权限后重试回滚。",
                            "Check directory permissions and retry rollback.",
                        )
                    })?;
                }
                atomic_write_bytes(path, bytes).map_err(|err| {
                    patch_error_with_hint(
                        "PATCH_IO_ROLLBACK_FAILED",
                        format!("回滚写入失败：{} ({err})", path.display()),
                        format!("Rollback failed to write file: {} ({err})", path.display()),
                        "请手动恢复文件并检查磁盘与权限状态。",
                        "Recover files manually and check disk/permission state.",
                    )
                })?;
            }
            None => {
                if path.exists() {
                    fs::remove_file(path).map_err(|err| {
                        patch_error_with_hint(
                            "PATCH_IO_ROLLBACK_FAILED",
                            format!("回滚删除失败：{} ({err})", path.display()),
                            format!("Rollback failed to remove file: {} ({err})", path.display()),
                            "请手动检查并恢复目标文件状态。",
                            "Check and recover target file state manually.",
                        )
                    })?;
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
        return Err(patch_error_with_hint(
            "PATCH_TARGET_IS_DIRECTORY",
            format!("路径是目录，不支持文本补丁：{}", path.display()),
            format!(
                "Path is a directory; text patch is not supported: {}",
                path.display()
            ),
            "请改用文件路径，目录不支持文本补丁。",
            "Use a file path instead; directory paths are not supported for text patches.",
        ));
    }
    let content = fs::read_to_string(path).map_err(|err| {
        patch_error_with_hint(
            "PATCH_IO_READ_FAILED",
            format!("读取文件失败：{} ({err})", path.display()),
            format!("Failed to read file: {} ({err})", path.display()),
            "请检查文件编码、权限与路径可访问性后重试。",
            "Check file encoding, permissions, and path accessibility before retrying.",
        )
    })?;
    Ok(Some(content))
}

pub(super) fn split_lines(content: &str) -> Vec<String> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
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

fn build_add_file_diff_block(lines: &[String]) -> FileDiffBlock {
    FileDiffBlock {
        header: "new file".to_string(),
        start_line_before: 0,
        end_line_before: 0,
        start_line_after: 1,
        end_line_after: lines.len(),
        lines: lines
            .iter()
            .enumerate()
            .map(|(index, line)| FileDiffLine {
                kind: "add",
                old_line: None,
                new_line: Some(index + 1),
                text: line.clone(),
            })
            .collect(),
    }
}

fn build_delete_file_diff_block(source: &str) -> FileDiffBlock {
    let lines = split_lines(source);
    FileDiffBlock {
        header: "deleted file".to_string(),
        start_line_before: 1,
        end_line_before: lines.len(),
        start_line_after: 0,
        end_line_after: 0,
        lines: lines
            .iter()
            .enumerate()
            .map(|(index, line)| FileDiffLine {
                kind: "delete",
                old_line: Some(index + 1),
                new_line: None,
                text: line.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
fn apply_update_chunks(
    source: &str,
    chunks: &[UpdateChunk],
    path: &str,
    cancel_probe: Option<&PatchCancelProbe>,
) -> Result<String> {
    apply_update_chunks_with_diff_engine(source, chunks, path, cancel_probe)
        .map(|(content, _)| content)
}

#[cfg(test)]
enum ChunkRangeSearchResult {
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
fn find_chunk_range(
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
        .map(|line| strip_display_line_number(line).unwrap_or_else(|| line.clone()))
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

fn analyze_update_chunk_effect(chunk: &UpdateChunk) -> UpdateChunkEffect {
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

fn build_patch_no_effect_hint(
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

pub(super) fn build_context_not_found_hint(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::monitor::MonitorState;
    use crate::storage::SqliteStorage;
    use serde_json::json;
    use std::sync::Arc;
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

    fn create_monitor_for_tests(workspace_root: &Path) -> Arc<MonitorState> {
        let db_path = workspace_root.join("monitor-tests.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let config = Config::default();
        Arc::new(MonitorState::new(
            storage,
            config.observability.clone(),
            workspace_root.to_string_lossy().to_string(),
        ))
    }

    #[test]
    fn build_patch_error_result_returns_structured_code_and_hint() {
        let err = patch_error_with_hint(
            "PATCH_TEST_CODE",
            "测试错误",
            "test error",
            "测试提示",
            "test hint",
        );
        let result = build_patch_error_result(err);
        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            result
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("error_code"))
                .and_then(Value::as_str),
            Some("PATCH_TEST_CODE")
        );
        assert!(result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("hint"))
            .and_then(Value::as_str)
            .is_some_and(|hint| !hint.trim().is_empty()));
        assert_eq!(
            result
                .get("error_meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("code"))
                .and_then(Value::as_str),
            Some("PATCH_TEST_CODE")
        );
    }

    #[test]
    fn build_patch_error_result_falls_back_to_unknown_code() {
        let result = build_patch_error_result(anyhow::anyhow!("unexpected boom"));
        assert_eq!(result.get("ok").and_then(Value::as_bool), Some(false));
        assert_eq!(
            result
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("error_code"))
                .and_then(Value::as_str),
            Some("PATCH_UNKNOWN")
        );
        assert!(result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("detail"))
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.contains("unexpected boom")));
        assert_eq!(
            result
                .get("error_meta")
                .and_then(Value::as_object)
                .and_then(|meta| meta.get("code"))
                .and_then(Value::as_str),
            Some("PATCH_UNKNOWN")
        );
    }

    #[test]
    fn build_patch_error_result_preserves_patch_no_effect_code() {
        let err = patch_error_with_hint(
            "PATCH_NO_EFFECT",
            "补丁没有产生实际修改",
            "Patch produced no effective change",
            "请确认新增与删除行确实不同。",
            "Ensure added and deleted lines are actually different.",
        );
        let result = build_patch_error_result(err);
        assert_eq!(
            result.pointer("/data/error_code").and_then(Value::as_str),
            Some("PATCH_NO_EFFECT")
        );
        assert_eq!(
            result.pointer("/error_meta/code").and_then(Value::as_str),
            Some("PATCH_NO_EFFECT")
        );
    }

    #[test]
    fn summarize_patch_ops_counts_actions_for_dry_run_preview() {
        let add_target = PathBuf::from("/tmp/add.txt");
        let del_target = PathBuf::from("/tmp/del.txt");
        let old_target = PathBuf::from("/tmp/old.txt");
        let new_target = PathBuf::from("/tmp/new.txt");
        let summary = summarize_patch_ops(&[
            ResolvedPatchOp::Add {
                path: "add.txt".to_string(),
                target: add_target.clone(),
                lines: vec!["x".to_string()],
            },
            ResolvedPatchOp::Delete {
                path: "del.txt".to_string(),
                target: del_target.clone(),
            },
            ResolvedPatchOp::Update {
                path: "old.txt".to_string(),
                target: old_target.clone(),
                move_to_path: Some("new.txt".to_string()),
                move_to_target: Some(new_target.clone()),
                chunks: vec![UpdateChunk {
                    change_context: None,
                    lines: vec![],
                    end_of_file: false,
                }],
            },
        ]);
        assert_eq!(summary.added, 1);
        assert_eq!(summary.deleted, 1);
        assert_eq!(summary.updated, 1);
        assert_eq!(summary.moved, 1);
        assert_eq!(summary.hunks_applied, 1);
        assert_eq!(summary.file_summaries.len(), 3);
        assert_eq!(summary.changed_files.len(), 4);
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
        let output = apply_update_chunks("a\nb\nc\n", &[chunk], "demo.txt", None)
            .expect("chunk should apply");
        assert_eq!(output, "a\nx\nc\n");
    }

    #[test]
    fn apply_update_chunks_stops_when_cancelled() {
        let dir = create_temp_dir("patch-cancelled");
        let monitor = create_monitor_for_tests(&dir);
        monitor.register("sess_cancel", "tester", "", "q", false, false);
        assert!(monitor.cancel("sess_cancel"));
        let probe = PatchCancelProbe {
            monitor,
            session_id: "sess_cancel".to_string(),
        };
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
        let error = apply_update_chunks("a\nb\nc\n", &[chunk], "demo.txt", Some(&probe))
            .expect_err("cancelled patch should stop");
        let result = build_patch_error_result(error);
        assert_eq!(
            result.pointer("/error_meta/code").and_then(Value::as_str),
            Some("CANCELLED")
        );
    }

    #[test]
    fn apply_patch_ops_marks_no_effect_update_when_content_is_unchanged() {
        let dir = create_temp_dir("patch-no-effect");
        let file_path = dir.join("demo.py");
        fs::write(&file_path, "print('hello')\n").expect("write source file");

        let summary = apply_patch_ops(
            vec![ResolvedPatchOp::Update {
                path: "demo.py".to_string(),
                target: file_path.clone(),
                move_to_path: None,
                move_to_target: None,
                chunks: vec![UpdateChunk {
                    change_context: None,
                    lines: vec![
                        ChunkLine {
                            kind: ChunkLineKind::Delete,
                            text: "print('hello')".to_string(),
                        },
                        ChunkLine {
                            kind: ChunkLineKind::Add,
                            text: "print('hello')".to_string(),
                        },
                    ],
                    end_of_file: false,
                }],
            }],
            None,
        )
        .expect("no-effect patch should still summarize");

        assert!(summary.changed_files.is_empty());
        assert_eq!(summary.hunks_applied, 0);
        assert_eq!(summary.no_effect_updates, vec!["demo.py".to_string()]);
        assert!(summary.file_summaries.is_empty());
        let content = fs::read_to_string(&file_path).expect("read source file");
        assert_eq!(content, "print('hello')\n");

        let result = build_patch_error_result(anyhow::anyhow!(
            PatchToolError::new(
                "PATCH_NO_EFFECT",
                "补丁没有产生任何实际修改：demo.py".to_string(),
                Some(
                    "这次补丁只有上下文，没有任何真正的新增/删除行。请确认每个 Update File 至少包含一对 `-旧行` / `+新行`，或者把真正改动并回同一个 hunk。".to_string()
                ),
                true,
            )
        ));
        let hint = result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("hint"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            hint.contains("没有任何真正的新增/删除行")
                || hint.contains("at least one `-old_line` / `+new_line` pair"),
            "unexpected hint: {hint}"
        );
    }

    #[test]
    fn apply_update_chunks_matches_chunks_in_forward_order_only() {
        let chunk_late = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "line3".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "LINE3".to_string(),
                },
            ],
            end_of_file: false,
        };
        let chunk_early = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "line1".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "LINE1".to_string(),
                },
            ],
            end_of_file: false,
        };

        let output = apply_update_chunks(
            "line1\nline2\nline3\n",
            &[chunk_early, chunk_late],
            "demo.txt",
            None,
        )
        .expect("forward-ordered chunks should apply");
        assert_eq!(output, "LINE1\nline2\nLINE3\n");
    }

    #[test]
    fn apply_update_chunks_matches_later_chunks_against_original_snapshot() {
        let chunk_title = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "# Old Title".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "# New Title".to_string(),
                },
            ],
            end_of_file: false,
        };
        let chunk_body = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "line2".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "LINE2".to_string(),
                },
            ],
            end_of_file: false,
        };

        let output = apply_update_chunks(
            "# Old Title\nline1\nline2\nline3\n",
            &[chunk_title, chunk_body],
            "demo.txt",
            None,
        )
        .expect("later chunks should still match original snapshot");
        assert_eq!(output, "# New Title\nline1\nLINE2\nline3\n");
    }

    #[test]
    fn apply_update_chunks_reports_ambiguous_global_fallback_match() {
        let chunk_tail = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "end".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "END".to_string(),
                },
            ],
            end_of_file: false,
        };
        let chunk_ambiguous = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "dup".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "DUP".to_string(),
                },
            ],
            end_of_file: false,
        };

        let error = apply_update_chunks(
            "dup\nx\ndup\nend\n",
            &[chunk_tail, chunk_ambiguous],
            "demo.txt",
            None,
        )
        .expect_err("ambiguous fallback should be rejected");
        let message = error.to_string();
        assert!(message.contains("找不到匹配上下文") || message.contains("no matching context"));
    }

    #[test]
    fn apply_patch_ops_rejects_add_when_file_exists() {
        let dir = create_temp_dir("apply-patch-add-exists");
        let existing = dir.join("existing.txt");
        fs::write(&existing, "old\n").expect("seed file should be written");

        let result = apply_patch_ops(
            vec![ResolvedPatchOp::Add {
                path: "existing.txt".to_string(),
                target: existing,
                lines: vec!["new".to_string()],
            }],
            None,
        );

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
        let result = apply_patch_ops(
            vec![ResolvedPatchOp::Update {
                path: "source.txt".to_string(),
                target: source,
                move_to_path: Some("destination.txt".to_string()),
                move_to_target: Some(destination),
                chunks: vec![chunk],
            }],
            None,
        );

        assert!(result.is_err());
        let message = result
            .expect_err("should reject occupied move target")
            .to_string();
        assert!(message.contains("目标文件已存在") || message.contains("already exists"));

        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn extract_patch_input_unwraps_nested_json_string() {
        let args = json!({
            "input": "{\"input\":\"*** Begin Patch\\n*** Add File: demo.txt\\n+ok\\n*** End Patch\"}"
        });
        let extracted = extract_patch_input(&args).expect("nested input should unwrap");
        assert_eq!(
            extracted,
            "*** Begin Patch\n*** Add File: demo.txt\n+ok\n*** End Patch"
        );
    }

    #[test]
    fn extract_patch_input_unwraps_double_nested_json_string() {
        let args = json!({
            "input": "{\"input\":\"{\\\"input\\\":\\\"*** Begin Patch\\\\n*** Add File: demo.txt\\\\n+ok\\\\n*** End Patch\\\"}\"}"
        });
        let extracted = extract_patch_input(&args).expect("double nested input should unwrap");
        assert_eq!(
            extracted,
            "*** Begin Patch\n*** Add File: demo.txt\n+ok\n*** End Patch"
        );
    }

    #[test]
    fn parse_patch_accepts_file_op_first_payload() {
        let patch = r#"*** Update File: demo.txt
@@
-old
+new
*** End Patch"#;
        let ops = parse_patch(patch).expect("file-op-first payload should be wrapped");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[0].text, "old");
        assert!(matches!(chunks[0].lines[1].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[1].text, "new");
    }

    #[test]
    fn parse_patch_treats_unified_diff_hunk_header_as_plain_hunk() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@ -1,3 +1,3 @@
 context
-old
+new
*** End Patch"#;
        let ops = parse_patch(patch).expect("unified diff hunk header should be tolerated");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks[0].change_context, None);
        assert_eq!(chunks[0].lines.len(), 3);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Context));
        assert_eq!(chunks[0].lines[0].text, "context");
    }

    #[test]
    fn parse_patch_preserves_function_anchor_from_unified_diff_hunk_header() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@ -10,3 +10,3 @@ fn main()
 context
-old
+new
*** End Patch"#;
        let ops = parse_patch(patch).expect("unified diff function anchor should be tolerated");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks[0].change_context.as_deref(), Some("fn main()"));
    }

    #[test]
    fn parse_patch_accepts_add_file_with_update_style_header_and_plain_lines() {
        let patch = r#"*** Begin Patch
*** Add File: demo.txt
@@
+first
second
*** End Patch"#;
        let ops = parse_patch(patch).expect("add file should tolerate common update-style shape");
        let ParsedPatchOp::Add { lines, .. } = &ops[0] else {
            panic!("expected add op");
        };
        assert_eq!(lines, &vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn parse_patch_repairs_display_separator_in_update_chunk() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
-old
***
+new
*** End Patch"#;
        let ops = parse_patch(patch).expect("display separator should be ignored");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].lines.len(), 2);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[0].text, "old");
        assert!(matches!(chunks[0].lines[1].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[1].text, "new");
    }

    #[test]
    fn parse_patch_repairs_numbered_display_diff() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
12: old
13: keep
***
12: new
13: keep
*** End Patch"#;
        let ops = parse_patch(patch).expect("numbered display diff should be repaired");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].lines.len(), 4);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[0].text, "old");
        assert!(matches!(chunks[0].lines[1].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[1].text, "keep");
        assert!(matches!(chunks[0].lines[2].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[2].text, "new");
        assert!(matches!(chunks[0].lines[3].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[3].text, "keep");
    }

    #[test]
    fn parse_patch_repairs_unprefixed_blank_lines_in_update_chunk() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
 line1
-line2
+line2x

 line3
*** End Patch"#;
        let ops =
            parse_patch(patch).expect("raw blank lines should be normalized to context lines");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].lines.len(), 5);
        assert!(matches!(chunks[0].lines[3].kind, ChunkLineKind::Context));
        assert_eq!(chunks[0].lines[3].text, "");
    }

    #[test]
    fn parse_patch_ignores_blank_separators_between_update_chunks() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
-line1
+line1x
@@

@@
 line2
*** End Patch"#;
        let ops =
            parse_patch(patch).expect("blank separators between chunk headers should be ignored");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].lines.len(), 2);
        assert_eq!(chunks[1].lines.len(), 1);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[0].text, "line1");
        assert!(matches!(chunks[0].lines[1].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[1].text, "line1x");
        assert!(matches!(chunks[1].lines[0].kind, ChunkLineKind::Context));
        assert_eq!(chunks[1].lines[0].text, "line2");
    }

    #[test]
    fn parse_patch_reports_actionable_hint_for_empty_update_line() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@

*** End Patch"#;
        let error = parse_patch(patch).expect_err("invalid empty update line should fail");
        let result = build_patch_error_result(error);
        let hint = result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("hint"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            hint.contains("blank lines also require a prefix")
                || hint.contains("空白行也必须带前缀"),
            "unexpected hint: {hint}"
        );
    }

    #[test]
    fn parse_patch_accepts_indented_update_header() {
        let patch = r#"*** Begin Patch
 *** Update File: demo.txt
@@
 line1
-line2
+line2x
*** End Patch"#;
        let ops = parse_patch(patch).expect("indented update header should be recovered");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Context));
        assert_eq!(chunks[0].lines[0].text, "line1");
    }

    #[test]
    fn parse_patch_accepts_plain_source_lines_inside_hunk_as_context() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
plain context
-old
+new
*** End Patch"#;
        let ops = parse_patch(patch).expect("plain source lines should be recovered as context");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Context));
        assert_eq!(chunks[0].lines[0].text, "plain context");
    }

    #[test]
    fn apply_update_chunks_not_found_hint_includes_nearby_context_details() {
        let chunk = UpdateChunk {
            change_context: Some("missing-anchor".to_string()),
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "line-2".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "line-4".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "line-2-updated".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "line-4-updated".to_string(),
                },
            ],
            end_of_file: false,
        };
        let error = apply_update_chunks(
            "line-1\nline-2\nline-3\nline-x\n",
            &[chunk],
            "demo.txt",
            None,
        )
        .expect_err("should fail when context is missing");
        let result = build_patch_error_result(error);
        let hint = result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("hint"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            hint.contains("邻近源码")
                || hint.contains("Nearby source")
                || hint.contains("Expected old snippet"),
            "unexpected hint: {hint}"
        );
        assert!(
            hint.contains("line-4") || hint.contains("line-x") || hint.contains("Mismatch samples"),
            "hint should include concrete line diff context: {hint}"
        );
    }

    #[test]
    fn apply_update_chunks_not_found_hint_explains_empty_hunk_duplicate_anchor_case() {
        let source = "line1\nax.axis('off')\n\n# Body\nbody = 1\n";
        let chunks = vec![
            UpdateChunk {
                change_context: None,
                lines: vec![
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "ax.axis('off')".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "# Body".to_string(),
                    },
                ],
                end_of_file: false,
            },
            UpdateChunk {
                change_context: None,
                lines: vec![
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "ax.axis('off')".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Add,
                        text: "# Canvas background".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Add,
                        text: "bg = 1".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "".to_string(),
                    },
                    ChunkLine {
                        kind: ChunkLineKind::Context,
                        text: "# Body".to_string(),
                    },
                ],
                end_of_file: false,
            },
        ];
        let error = apply_update_chunks(source, &chunks, "demo.txt", None)
            .expect_err("duplicate anchor after context-only hunk should fail with guidance");
        let result = build_patch_error_result(error);
        let hint = result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("hint"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            hint.contains("空 hunk")
                || hint.contains("上下文")
                || hint.contains("empty hunk")
                || hint.contains("context-only"),
            "unexpected hint: {hint}"
        );
        assert!(
            hint.contains("同一个 hunk")
                || hint.contains("删除前一个空")
                || hint.contains("single hunk")
                || hint.contains("Remove the earlier empty hunk"),
            "unexpected hint: {hint}"
        );
    }

    #[test]
    fn apply_update_chunks_not_found_hint_explains_insertion_without_trailing_anchor() {
        let source = "alpha\nanchor\nbody\nanchor\nbody\n";
        let chunk = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Context,
                    text: "anchor".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "# inserted".to_string(),
                },
            ],
            end_of_file: false,
        };
        let error = apply_update_chunks(source, &[chunk], "demo.txt", None)
            .expect_err("insertion without trailing anchor should produce a corrective hint");
        let result = build_patch_error_result(error);
        let hint = result
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("hint"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            hint.contains("尾部上下文")
                || hint.contains("trailing context")
                || hint.contains("尾部锚点")
                || hint.contains("trailing anchor"),
            "unexpected hint: {hint}"
        );
    }

    #[test]
    fn repair_strips_line_numbers_from_prefixed_context_lines() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
 1: hello
 2: world
-world
+WORLD
 4: end
*** End Patch"#;
        let ops = parse_patch(patch).expect("line numbers in context lines should be stripped");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].lines[0].text, "hello");
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Context));
        // " 2: world" strips to " world" (context), "-world" is delete with same text;
        // dedup removes the redundant context line, keeping only the delete.
        assert!(matches!(chunks[0].lines[1].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[1].text, "world");
        assert!(matches!(chunks[0].lines[2].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[2].text, "WORLD");
        assert_eq!(chunks[0].lines[3].text, "end");
        assert!(matches!(chunks[0].lines[3].kind, ChunkLineKind::Context));
    }

    #[test]
    fn find_chunk_range_falls_back_to_stripped_line_numbers() {
        let source = "hello\nworld\nfoo\n";
        let chunk = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Context,
                    text: "2: world".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "3: foo".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "3: bar".to_string(),
                },
            ],
            end_of_file: false,
        };
        let old_lines: Vec<String> = chunk
            .lines
            .iter()
            .filter(|l| l.kind != ChunkLineKind::Add)
            .map(|l| l.text.clone())
            .collect();
        let source_lines: Vec<String> = split_lines(source);
        let result = find_chunk_range(&source_lines, 0, &chunk, &old_lines);
        assert!(
            matches!(result, ChunkRangeSearchResult::Found(_)),
            "should find match after stripping line numbers"
        );
    }

    #[test]
    fn find_chunk_range_fuzzy_matches_with_whitespace_tolerance() {
        let source = "def hello():\n    print('hi')\n    return\n";
        let chunk = UpdateChunk {
            change_context: None,
            lines: vec![
                ChunkLine {
                    kind: ChunkLineKind::Context,
                    text: "def hello():".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Delete,
                    text: "  print('hi')".to_string(),
                },
                ChunkLine {
                    kind: ChunkLineKind::Add,
                    text: "  print('hello')".to_string(),
                },
            ],
            end_of_file: false,
        };
        let old_lines: Vec<String> = chunk
            .lines
            .iter()
            .filter(|l| l.kind != ChunkLineKind::Add)
            .map(|l| l.text.clone())
            .collect();
        let source_lines: Vec<String> = split_lines(source);
        let result = find_chunk_range(&source_lines, 0, &chunk, &old_lines);
        assert!(
            matches!(result, ChunkRangeSearchResult::Found(_)),
            "should find match with fuzzy whitespace tolerance"
        );
    }

    #[test]
    fn dedup_repaired_numbered_context_before_delete_removes_redundant_context() {
        let lines = vec![
            " hello".to_string(),
            "-hello".to_string(),
            "+HELLO".to_string(),
            " world".to_string(),
        ];
        let repaired_numbered_context = vec![true, false, false, false];
        let result =
            dedup_repaired_numbered_context_before_delete(&lines, &repaired_numbered_context);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], "-hello");
        assert_eq!(result[1], "+HELLO");
        assert_eq!(result[2], " world");
    }

    #[test]
    fn dedup_repaired_numbered_context_before_delete_keeps_non_duplicate_context() {
        let lines = vec![
            " above".to_string(),
            "-old_line".to_string(),
            "+new_line".to_string(),
            " below".to_string(),
        ];
        let repaired_numbered_context = vec![true, false, false, false];
        let result =
            dedup_repaired_numbered_context_before_delete(&lines, &repaired_numbered_context);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], " above");
        assert_eq!(result[1], "-old_line");
    }

    #[test]
    fn dedup_repaired_numbered_context_before_delete_keeps_user_authored_duplicate_context() {
        let lines = vec![
            " same".to_string(),
            "-same".to_string(),
            "+SAME".to_string(),
            " tail".to_string(),
        ];
        let repaired_numbered_context = vec![false, false, false, false];
        let result =
            dedup_repaired_numbered_context_before_delete(&lines, &repaired_numbered_context);
        assert_eq!(result, lines);
    }

    #[test]
    fn repair_handles_unprefixed_numbered_lines() {
        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
1: hello
2: world
-world
+WORLD
4: end
*** End Patch"#;
        let ops = parse_patch(patch).expect("unprefixed numbered lines should be repaired");
        let ParsedPatchOp::Update { chunks, .. } = &ops[0] else {
            panic!("expected update op");
        };
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0].lines[0].kind, ChunkLineKind::Context));
        assert_eq!(chunks[0].lines[0].text, "hello");
        assert!(matches!(chunks[0].lines[1].kind, ChunkLineKind::Delete));
        assert_eq!(chunks[0].lines[1].text, "world");
        assert!(matches!(chunks[0].lines[2].kind, ChunkLineKind::Add));
        assert_eq!(chunks[0].lines[2].text, "WORLD");
        assert!(matches!(chunks[0].lines[3].kind, ChunkLineKind::Context));
        assert_eq!(chunks[0].lines[3].text, "end");
    }
}
