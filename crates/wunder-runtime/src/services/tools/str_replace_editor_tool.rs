//! 子命令编辑形态（原 `str_replace_editor`），现为「文本编辑」工具的一个分支。
//!
//! 由 `text_edit` 在检测到非空 `command` 时分派过来。
//! - `view`：文件按 `cat -n` 风格带行号渲染（六位右对齐 + 两空格），目录列出
//!   最多两层、过滤隐藏项与 `node_modules`/`__pycache__`；
//! - `create`：仅在目标不存在时新建；
//! - `str_replace` 在统一入口与默认字面替换复用同一执行实现；
//! - `insert`：把 `new_string` 插入到第 `insert_line` 行之后；
//! - `undo_edit`：把目标文件恢复成最近一次快照（上一次写入前的内容），
//!   恢复前也存一份快照，所以回滚本身还能再撤回。
//!
//! 输出超过 `MAX_OUTPUT_CHARS` 时按字符截断并追加 `<response clipped>` 说明。
//! 路径沿用 workspace 记账，工作区外的允许根走能力 seam。
//!
//! 三个写类子命令（`create`/`str_replace`/`insert`）的 `data` 都带补丁家族的
//! diff 键（见 [`super::line_diff`]），与 `写入文件` 共用同一份词汇，渲染侧一个
//! 卡片模板即可；`view` 不写盘因此不带 diff。

use super::capabilities::{local_fs, FsCapability};
use super::context::collect_allow_roots;
use super::line_diff::{
    diff_added_file, diff_replaced, diff_source_too_large, insert_diff_fields, unified_diff,
    LineDiff, DIFF_UNAVAILABLE_OVERSIZE, MAX_UNIFIED_DIFF_LINES,
};
use super::{file_history, write_receipt};
use super::{
    build_model_tool_success,
    command_options::parse_dry_run,
    resolve_tool_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    ToolContext,
};
use crate::core::blocking;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// 当参数携带非空 `command` 时，走子命令编辑分支。
pub(crate) fn command_requested(args: &Value) -> bool {
    args.get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

/// Maximum returned characters before clipping (dsh default).
const MAX_OUTPUT_CHARS: usize = 16_000;

const TRUNCATED_MESSAGE: &str = "\n<response clipped>Use search_content to locate relevant lines, then view with a narrower view_range.";

const VALID_COMMANDS: [&str; 5] = ["view", "create", "insert", "undo_edit", "redo"];

struct EditorOutcome {
    value: Value,
    mutated: bool,
}

fn fail(value: Value) -> EditorOutcome {
    EditorOutcome {
        value,
        mutated: false,
    }
}

fn ok(value: Value, mutated: bool) -> EditorOutcome {
    EditorOutcome { value, mutated }
}

fn editor_failure(message: &str, data: Value, code: &str, hint: &str) -> Value {
    build_failed_tool_result(
        message,
        data,
        ToolErrorMeta::new(code, Some(hint.to_string()), false, None),
        false,
    )
}

fn success(action: &str, summary: &str, data: Value) -> Value {
    build_model_tool_success(action, "completed", summary, data)
}

fn missing_param(key: &str, command: &str) -> Value {
    // 缺参提示直接给出可照做的补救动作：同一语义的文本参数在不同子命令里名字
    // 不同，只说「缺 new_string」会让模型按自己的猜测反复重试。
    let hint = match (command, key) {
        ("insert", "insert_line") => {
            "insert 需要整数 insert_line：0 表示文件开头，N 表示插到第 N 行之后。"
        }
        ("insert", "new_string") => "insert 需要要写入的文本 new_string（别名 file_text 亦可）。",
        ("create", "file_text") => "create 需要文件内容 file_text（别名 new_string 亦可）。",
        _ => "补充必需参数后重试。",
    };
    editor_failure(
        &format!("缺少参数「{}」（command={command}）。", key),
        json!({ "missing": key, "command": command }),
        "TOOL_SRE_MISSING_PARAM",
        hint,
    )
}

fn not_found(path: &str) -> Value {
    editor_failure(
        &format!("The path {path} does not exist. Please provide a valid path."),
        json!({ "path": path }),
        "FS_NOT_FOUND",
        "确认路径正确后重试；新建文件请改用 create。",
    )
}

fn not_regular_file(path: &str) -> Value {
    editor_failure(
        &format!(
            "The path {path} is a directory and only the `view` command can be used on directories"
        ),
        json!({ "path": path }),
        "FS_NOT_REGULAR_FILE",
        "目录只支持 view；文件编辑请给出文件路径。",
    )
}

pub(crate) async fn str_replace_editor(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    // `undo_edit` / `redo` 依赖本机 temp/.history 快照，而沙盒的文件写入在沙盒侧完成、
    // 不落这套快照；直接下发给它只会得到「未知命令」，所以这一条始终走本机。
    let host_only = args
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| value == "undo_edit" || value == "redo");
    if !host_only {
        if let Some(result) =
            super::execute_in_sandbox(context, super::edit_tool::TOOL_EDIT, args).await
        {
            if super::edit_tool::sandbox_edit_mutated(&result, args) {
                context.workspace.mark_tree_dirty(context.workspace_id);
            }
            return Ok(result);
        }
    }

    let command = args
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(command) = command else {
        return Ok(missing_param("command", "文本编辑"));
    };
    if !VALID_COMMANDS.contains(&command) {
        return Ok(editor_failure(
            &format!(
                "Unknown command `{command}`. Allowed options are: `view`, `create`, `str_replace`, `insert`, `undo_edit`, `redo`."
            ),
            json!({ "command": command }),
            "TOOL_SRE_COMMAND_INVALID",
            "使用 view、create、str_replace、insert、undo_edit 或 redo。",
        ));
    }
    // 别名已在统一入口归一化，此处只处理主参数。
    let path = args
        .get("file_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(path) = path else {
        return Ok(editor_failure(
            "Parameter `file_path` must be a non-empty string.",
            json!({ "path_required": true }),
            "TOOL_SRE_PATH_REQUIRED",
            "提供非空字符串 file_path。",
        ));
    };

    let command = command.to_string();
    let path = path.to_string();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let allow_roots = collect_allow_roots(context);
    let payload = args.clone();
    let path_for_run = path.clone();
    let outcome = blocking::run_fs("tools.file.str_replace_editor", move || {
        run_editor(
            workspace,
            user_id,
            allow_roots,
            &command,
            &path_for_run,
            &payload,
        )
    })
    .await;

    match outcome {
        Ok(editor) => {
            if editor.mutated {
                context.workspace.mark_tree_dirty(context.workspace_id);
            }
            Ok(editor.value)
        }
        Err(err) => Ok(editor_failure(
            &format!("文本编辑失败：{err}"),
            json!({ "path": path }),
            "TOOL_SRE_FAILED",
            "确认路径权限与文件状态后重试。",
        )),
    }
}

fn run_editor(
    workspace: Arc<WorkspaceManager>,
    user_id: String,
    allow_roots: Vec<PathBuf>,
    command: &str,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    let ws = workspace.as_ref();
    let target = resolve_tool_path(ws, &user_id, path, &allow_roots)?;
    match command {
        "view" => view_command(ws, &user_id, &target, path, args),
        "create" => create_command(ws, &user_id, &target, path, args),
        "insert" => insert_command(ws, &user_id, &target, path, args),
        "undo_edit" => undo_edit_command(ws, &user_id, &target, path, args),
        "redo" => redo_edit_command(ws, &user_id, &target, path, args),
        _ => Ok(fail(editor_failure(
            &format!("Unknown command `{command}`."),
            json!({ "command": command }),
            "TOOL_SRE_COMMAND_INVALID",
            "使用 view、create、str_replace、insert、undo_edit 或 redo。",
        ))),
    }
}

fn view_command(
    _ws: &WorkspaceManager,
    _user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    if !target.exists() {
        return Ok(fail(not_found(path)));
    }
    let view_range = match parse_view_range(args) {
        Ok(range) => range,
        Err(message) => {
            return Ok(fail(editor_failure(
                &message,
                json!({ "path": path }),
                "TOOL_SRE_INVALID_VIEW_RANGE",
                "view_range 需为两个整数，例如 [11, 12] 或 [start, -1]。",
            )));
        }
    };
    if target.is_dir() {
        if view_range.is_some() {
            return Ok(fail(editor_failure(
                "The `view_range` parameter is not allowed when `path` points to a directory.",
                json!({ "path": path }),
                "TOOL_SRE_VIEW_RANGE_ON_DIR",
                "目录视图不接受 view_range。",
            )));
        }
        let listing = list_directory(target, path, MAX_OUTPUT_CHARS);
        return Ok(ok(
            success(
                "view",
                &format!("Viewed directory {path}."),
                json!({ "path": path, "kind": "directory", "content": listing }),
            ),
            false,
        ));
    }
    if !target.is_file() {
        return Ok(fail(not_regular_file(path)));
    }
    let fs_cap = local_fs();
    let content = fs_cap.read_text(target)?;
    let rendered = match format_file_view(path, &content, view_range) {
        Ok(rendered) => rendered,
        Err(message) => {
            return Ok(fail(editor_failure(
                &message,
                json!({ "path": path }),
                "TOOL_SRE_INVALID_VIEW_RANGE",
                "view_range 需为两个整数，例如 [11, 12] 或 [start, -1]。",
            )));
        }
    };
    Ok(ok(
        success(
            "view",
            &format!("Viewed {path}."),
            json!({ "path": path, "kind": "file", "content": rendered }),
        ),
        false,
    ))
}

fn create_command(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    let file_text = match args.get("file_text").and_then(Value::as_str) {
        Some(value) => value.to_string(),
        None => return Ok(fail(missing_param("file_text", "create"))),
    };
    if target.exists() {
        return Ok(fail(editor_failure(
            &format!(
                "File already exists at: {path}. Cannot overwrite files using command `create`."
            ),
            json!({ "path": path }),
            "TOOL_SRE_FILE_EXISTS",
            "删除或改名后重试，或改用 str_replace/insert。",
        )));
    }
    let dry_run = parse_dry_run(args);
    let verify = parse_verify(args);
    if !dry_run {
        write_target(ws, user_id, target, path, &file_text)?;
    }
    let diff = diff_added_file(&file_text);
    let data = build_edit_data(path, "add", &diff, None);
    let data = with_receipt(
        path, "", &file_text, &diff, None, None, verify, data,
    );
    Ok(write_outcome("create", path, data, dry_run, true))
}

/// 组装写类子命令的结果 `data`：保留既有 `path` 键，追加 diff 家族键。
/// `action` 沿用补丁家族的 `add`/`update`；`unavailable` 是有界守卫放弃 diff
/// 时的短原因串，落在 `data.diff_unavailable`。
fn build_edit_data(
    path: &str,
    action: &str,
    diff: &LineDiff,
    unavailable: Option<&'static str>,
) -> Value {
    let mut fields = Map::<String, Value>::new();
    insert_diff_fields(&mut fields, action, path, diff, unavailable);
    if let Some(reason) = unavailable {
        fields.insert("diff_unavailable".to_string(), json!(reason));
    }
    let mut data = json!({ "path": path });
    if let Some(map) = data.as_object_mut() {
        map.extend(fields);
    }
    data
}

fn insert_command(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    let insert_line = match args.get("insert_line") {
        Some(Value::Number(number)) => number.as_i64(),
        _ => None,
    };
    let Some(insert_line) = insert_line else {
        return Ok(fail(missing_param("insert_line", "insert")));
    };
    let new_str = match args.get("new_string").and_then(Value::as_str) {
        Some(value) => value,
        None => return Ok(fail(missing_param("new_string", "insert"))),
    };
    if !target.exists() {
        return Ok(fail(not_found(path)));
    }
    if target.is_dir() {
        return Ok(fail(not_regular_file(path)));
    }
    let fs_cap = local_fs();
    let before = fs_cap.read_text(target)?;
    let total_lines = before.lines().count();
    if insert_line < 0 || insert_line as u64 > total_lines as u64 {
        return Ok(fail(editor_failure(
            &format!("insert_line={insert_line} 超出范围 [0, {total_lines}]；未修改文件。"),
            json!({ "path": path, "total_lines": total_lines }),
            "TOOL_SRE_INVALID_INSERT_LINE",
            "先用 view 查看当前行号；0 表示文件开头。",
        )));
    }
    let index = insert_line as usize;
    let offset = if index == 0 {
        0
    } else {
        before
            .match_indices('\n')
            .nth(index - 1)
            .map(|(at, _)| at + 1)
            .unwrap_or(before.len())
    };
    let normalized = super::edit_tool::match_text_newlines(&before, new_str);
    let mut inserted = normalized.into_owned();
    if !inserted.is_empty() {
        let newline = if before.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if offset > 0 && !before[..offset].ends_with('\n') {
            inserted.insert_str(0, newline);
        }
        if offset < before.len() || before.ends_with('\n') {
            if !inserted.ends_with('\n') {
                inserted.push_str(newline);
            }
        }
    }
    let mut updated = String::with_capacity(before.len() + inserted.len());
    updated.push_str(&before[..offset]);
    updated.push_str(&inserted);
    updated.push_str(&before[offset..]);
    let dry_run = parse_dry_run(args);
    let verify = parse_verify(args);
    let changed = updated != before;
    // 覆盖前快照：写坏了能 diff 回滚；失败只是没有历史，不影响写入。
    let snapshot = if !dry_run && changed {
        file_history::snapshot(target, &ws.workspace_root(user_id), &before)
    } else {
        None
    };
    if !dry_run && changed {
        write_target(ws, user_id, target, path, &updated)?;
    }
    let (diff, unavailable) = if diff_source_too_large(&before, &updated) {
        (LineDiff::default(), Some(DIFF_UNAVAILABLE_OVERSIZE))
    } else {
        (diff_replaced(&before, &updated), None)
    };
    let data = build_edit_data(path, "update", &diff, unavailable);
    let data = with_receipt(
        path,
        &before,
        &updated,
        &diff,
        unavailable,
        snapshot.as_ref(),
        verify,
        data,
    );
    Ok(write_outcome("insert", path, data, dry_run, changed))
}

/// 回滚：把目标文件写回最近一次快照（即上一次写入前的内容）。
///
/// 语义是「退一步」而不是「清空历史」：回滚前先把当前内容存成新快照，
/// 所以连续调用会一步步往回走，误回滚也能再 undo 回来。
/// 撤销：把目标文件写回栈里上一态的内容，只移动游标、不压新快照。
///
/// 与「回滚到最近一版」不同，连续调用会一路往回退（C → B → A），退到最初
/// 之后报 `TOOL_SRE_AT_OLDEST`（有历史、只是已经在最早一态）；撤销之后再写入
/// 会砍掉 redo 分支，所以两条时间线不会互串。
fn undo_edit_command(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    if target.is_dir() {
        return Ok(fail(not_regular_file(path)));
    }
    let workspace_root = ws.workspace_root(user_id);
    let before = read_current(target)?;
    let step = match file_history::step_back(target, &workspace_root, &before) {
        Ok(step) => step,
        Err(block) => return Ok(fail(history_block(path, block))),
    };
    apply_history_step(ws, user_id, target, path, args, &before, step, "undo_edit")
}

fn redo_edit_command(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    if target.is_dir() {
        return Ok(fail(not_regular_file(path)));
    }
    let workspace_root = ws.workspace_root(user_id);
    let before = read_current(target)?;
    let step = match file_history::step_forward(target, &workspace_root) {
        Ok(step) => step,
        Err(block) => return Ok(fail(history_block(path, block))),
    };
    apply_history_step(ws, user_id, target, path, args, &before, step, "redo")
}

fn read_current(target: &Path) -> Result<String> {
    let fs_cap = local_fs();
    if target.is_file() {
        fs_cap.read_text(target)
    } else {
        Ok(String::new())
    }
}

/// 撤销/重做共用的落盘收尾：写回内容、挪游标、回执与 diff。
///
/// 游标只在真正写盘后提交；`dry_run` 连游标都不动，预演不会破坏栈。
fn apply_history_step(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
    before: &str,
    step: file_history::StepBack,
    action: &str,
) -> Result<EditorOutcome> {
    let restored = step.content.clone();
    let dry_run = parse_dry_run(args);
    let verify = parse_verify(args);
    let changed = restored != before;
    if !dry_run {
        if changed {
            write_target(ws, user_id, target, path, &restored)?;
        }
        // 内容已经等于目标态时也要挪游标，否则下一次进退会卡在原地。
        let _ = file_history::commit_cursor(target, &ws.workspace_root(user_id), step.cursor);
    }
    let (diff, unavailable) = if diff_source_too_large(before, &restored) {
        (LineDiff::default(), Some(DIFF_UNAVAILABLE_OVERSIZE))
    } else {
        (diff_replaced(before, &restored), None)
    };
    let data = build_edit_data(path, "update", &diff, unavailable);
    let mut data = with_receipt(path, before, &restored, &diff, unavailable, None, verify, data);
    if let Some(map) = data.as_object_mut() {
        map.insert("restored_from".to_string(), json!(step.display));
        map.insert("cursor".to_string(), json!(step.cursor));
    }
    Ok(write_outcome(action, path, data, dry_run, changed))
}

/// 退不动时按原因分派：栈底和「根本没有历史」是两件事，提示不能共用。
fn history_block(path: &str, block: file_history::StepBlock) -> Value {
    match block {
        file_history::StepBlock::AtOldest { revisions } => at_oldest(path, revisions),
        file_history::StepBlock::AtNewest => no_redo(path),
        file_history::StepBlock::Corrupt { display } => corrupt_snapshot(path, &display),
        file_history::StepBlock::NoHistory => no_history(path),
    }
}

/// 有历史但已经退到最早一态：文件没被改动，历史也还在。
fn at_oldest(path: &str, revisions: u32) -> Value {
    editor_failure(
        &format!("{path} 已退回最早的可用版本；未修改文件。"),
        json!({ "path": path, "history_at_oldest": true, "revisions": revisions }),
        "TOOL_SRE_AT_OLDEST",
        "历史里最早的一版就是当前内容，继续撤销不会有变化；要更早的内容只能靠外部备份。",
    )
}

/// 快照内容与文件名里的短 sha 不符：按损坏处理，已删除，绝不写回文件。
fn corrupt_snapshot(path: &str, snapshot: &str) -> Value {
    editor_failure(
        &format!("{path} 的快照内容与文件名不符，已按损坏删除；未修改文件。"),
        json!({ "path": path, "history_corrupt": snapshot }),
        "TOOL_SRE_HISTORY_CORRUPT",
        "历史目录被外部改过；重新写入一次即可重建这一版之后的历史。",
    )
}

fn no_history(path: &str) -> Value {
    editor_failure(
        &format!("{path} 没有可回滚的快照；未修改文件。"),
        json!({ "path": path, "history_missing": true }),
        "TOOL_SRE_NO_HISTORY",
        "快照只在改写已有文件时生成：直接写回目标内容即可，不需要回滚。",
    )
}

fn no_redo(path: &str) -> Value {
    editor_failure(
        &format!("{path} 没有可重做的步骤；未修改文件。"),
        json!({ "path": path, "redo_missing": true }),
        "TOOL_SRE_NO_REDO",
        "redo 只在撤销之后可用；撤销之后再写入会丢弃 redo 分支，需要重新改一次。",
    )
}


fn write_outcome(
    action: &str,
    path: &str,
    mut data: Value,
    dry_run: bool,
    changed: bool,
) -> EditorOutcome {
    // 预览标记必须落进 `data`：模型与投影层都靠它区分「没写盘」和「写盘但
    // 内容没变」，缺了就只能被读成后者（changed: true）。
    data["dry_run"] = json!(dry_run);
    if !changed {
        data["changed_files"] = json!(0);
        data["updated"] = json!(0);
    }
    let state = if dry_run {
        "dry_run"
    } else if changed {
        "completed"
    } else {
        "unchanged"
    };
    let summary = if dry_run {
        format!("Validated {action} on {path}; no content written.")
    } else if changed {
        format!("Applied {action} to {path}.")
    } else {
        format!("No content change in {path}.")
    };
    ok(
        build_model_tool_success(action, state, summary, data),
        !dry_run && changed,
    )
}

/// 写类子命令的公共收尾：回执（行数/sha256/符号/语法）+ unified diff + 历史快照。
///
/// 与 `写入文件`、字面替换共用同一份口径，模型不必因为换了子命令就再回读一次。
fn with_receipt(
    path: &str,
    before: &str,
    after: &str,
    diff: &LineDiff,
    unavailable: Option<&'static str>,
    snapshot: Option<&file_history::Snapshot>,
    verify: bool,
    mut data: Value,
) -> Value {
    let receipt = write_receipt::build(path, after, verify);
    let (unified, unified_truncated) =
        unified_diff(path, before, after, diff, MAX_UNIFIED_DIFF_LINES);
    if let Some(map) = data.as_object_mut() {
        if let Some(snapshot) = snapshot {
            map.insert("history".to_string(), json!(snapshot.display));
            map.insert("history_rev".to_string(), json!(snapshot.rev));
        }
        write_receipt::insert_fields(map, &receipt);
        map.insert("unified_diff".to_string(), json!(unified));
        map.insert(
            "unified_diff_truncated".to_string(),
            json!(unified_truncated),
        );
        if let Some(reason) = unavailable {
            map.insert("diff_unavailable".to_string(), json!(reason));
        }
    }
    data
}

fn parse_verify(args: &Value) -> bool {
    args.get("verify").and_then(Value::as_bool).unwrap_or(true)
}


fn write_target(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    content: &str,
) -> Result<()> {
    let fs_cap = local_fs();
    let workspace_root = ws.workspace_root(user_id);
    let default_target = ws.resolve_path(user_id, path)?;
    if is_within_root(&workspace_root, target)
        && normalize_path_for_compare(&normalize_target_path(target))
            == normalize_path_for_compare(&normalize_target_path(&default_target))
    {
        ws.write_file(user_id, path, content, true)?;
    } else {
        if let Some(parent) = target.parent() {
            fs_cap.create_dir_all(parent)?;
        }
        fs_cap.write_text(target, content)?;
    }
    Ok(())
}

fn parse_view_range(args: &Value) -> std::result::Result<Option<Vec<i64>>, String> {
    match args.get("view_range") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut range = Vec::with_capacity(items.len());
            for item in items {
                match item.as_i64() {
                    Some(value) => range.push(value),
                    None => {
                        return Err("Invalid `view_range`. It should be a list of two integers."
                            .to_string());
                    }
                }
            }
            if range.len() != 2 {
                return Err(
                    "Invalid `view_range`. It should be a list of two integers.".to_string()
                );
            }
            Ok(Some(range))
        }
        Some(_) => Err("Invalid `view_range`. It should be a list of two integers.".to_string()),
    }
}

fn format_file_view(
    path: &str,
    content: &str,
    view_range: Option<Vec<i64>>,
) -> std::result::Result<String, String> {
    let total = content.lines().count();
    let (start, end) = if let Some(range) = view_range {
        if range.len() != 2 {
            return Err("view_range must be [start, end].".to_string());
        }
        let (start, end) = (range[0], range[1]);
        if start < 1
            || start as u64 > total as u64
            || (end != -1 && (end < start || end as u64 > total as u64))
        {
            return Err(format!(
                "Invalid view_range=[{start}, {end}]; file has {total} lines. Use end=-1 for EOF."
            ));
        }
        (start as usize, if end == -1 { total } else { end as usize })
    } else {
        (1, total)
    };
    let mut output = format!(
        "Here's the content of {path} with line numbers (which has a total of {total} lines)"
    );
    if start != 1 || end != total {
        output.push_str(&format!(" with view_range=[{start}, {end}]"));
    }
    output.push_str(":\n");
    // Render only the requested window and stop at the output budget, even
    // for a single very long line. No full-file line vector or rendered copy.
    let mut remaining = MAX_OUTPUT_CHARS.saturating_sub(output.chars().count());
    for (index, line) in content
        .lines()
        .enumerate()
        .skip(start - 1)
        .take(end.saturating_sub(start - 1))
    {
        for part in [&format!("{:>6}  ", index + 1), line, "\n"] {
            let mut chars = part.chars();
            while remaining > 0 {
                let Some(ch) = chars.next() else { break };
                output.push(ch);
                remaining -= 1;
            }
            if chars.next().is_some() {
                output.push_str(TRUNCATED_MESSAGE);
                return Ok(output);
            }
        }
    }
    Ok(output)
}

fn list_directory(root: &Path, display_base: &str, max_chars: usize) -> String {
    let base = display_base.trim_end_matches('/').to_string();
    let mut rows: Vec<(char, String)> = vec![('d', base.clone())];
    let walker = WalkDir::new(root)
        .min_depth(1)
        .max_depth(2)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            !(name.starts_with('.') || name == "node_modules" || name == "__pycache__")
        });
    for entry in walker.filter_map(|item| item.ok()) {
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let display = if base.is_empty() {
            relative
        } else {
            format!("{base}/{relative}")
        };
        let kind = if entry.file_type().is_dir() {
            'd'
        } else if entry.file_type().is_file() {
            'f'
        } else {
            '?'
        };
        rows.push((kind, display));
    }
    rows.sort_by(|left, right| left.1.cmp(&right.1));
    let listing = rows
        .iter()
        .map(|(kind, path)| format!("{kind}\t{path}"))
        .collect::<Vec<_>>()
        .join("\n");
    let listing = maybe_truncate(&format!("{listing}\n"), max_chars);
    format!(
        "Here're the files and directories up to 2 levels deep in {display_base}, excluding hidden items, node_modules, and Python cache directories:\n{listing}\n"
    )
}

fn maybe_truncate(content: &str, max_chars: usize) -> String {
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let truncated: String = content.chars().take(max_chars).collect();
    format!("{truncated}{TRUNCATED_MESSAGE}")
}

#[cfg(test)]
#[path = "str_replace_editor_tests.rs"]
mod editor_tests;
