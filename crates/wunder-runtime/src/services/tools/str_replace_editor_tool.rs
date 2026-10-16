//! 子命令编辑形态（原 `str_replace_editor`），现为「文本编辑」工具的一个分支。
//!
//! 由 `text_edit` 在检测到非空 `command` 时分派过来。
//! - `view`：文件按 `cat -n` 风格带行号渲染（六位右对齐 + 两空格），目录列出
//!   最多两层、过滤隐藏项与 `node_modules`/`__pycache__`；
//! - `create`：仅在目标不存在时新建；
//! - `str_replace` 在统一入口与默认字面替换复用同一执行实现；
//! - `insert`：把 `new_string` 插入到第 `insert_line` 行之后。
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
    diff_added_file, diff_replaced, diff_source_too_large, insert_diff_fields, LineDiff,
    DIFF_UNAVAILABLE_OVERSIZE,
};
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

const VALID_COMMANDS: [&str; 3] = ["view", "create", "insert"];

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
    editor_failure(
        &format!("Parameter `{key}` is required for command: {command}"),
        json!({ "missing": key }),
        "TOOL_SRE_MISSING_PARAM",
        "补充必需参数后重试。",
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
    if let Some(result) =
        super::execute_in_sandbox(context, super::edit_tool::TOOL_EDIT, args).await
    {
        if super::edit_tool::sandbox_edit_mutated(&result, args) {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
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
                "Unknown command `{command}`. Allowed options are: `view`, `create`, `str_replace`, `insert`."
            ),
            json!({ "command": command }),
            "TOOL_SRE_COMMAND_INVALID",
            "使用 view、create、str_replace 或 insert。",
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
        _ => Ok(fail(editor_failure(
            &format!("Unknown command `{command}`."),
            json!({ "command": command }),
            "TOOL_SRE_COMMAND_INVALID",
            "使用 view、create、str_replace 或 insert。",
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
    if !dry_run {
        write_target(ws, user_id, target, path, &file_text)?;
    }
    Ok(write_outcome(
        "create",
        path,
        build_create_data(path, &file_text),
        dry_run,
        true,
    ))
}

/// 新建文件的 `data`：与 `写入文件` 命中不存在路径时逐字一致，整份内容一个
/// `new file` 块、每行 `kind:"add"`。热路径只扫一遍新内容，行数/文本各自有界。
fn build_create_data(path: &str, content: &str) -> Value {
    build_edit_data(path, "add", &diff_added_file(content), None)
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
    insert_diff_fields(&mut fields, action, path, diff);
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
    let changed = updated != before;
    if !dry_run && changed {
        write_target(ws, user_id, target, path, &updated)?;
    }
    Ok(write_outcome(
        "insert",
        path,
        build_insert_data(path, &before, &updated),
        dry_run,
        changed,
    ))
}

fn write_outcome(
    action: &str,
    path: &str,
    mut data: Value,
    dry_run: bool,
    changed: bool,
) -> EditorOutcome {
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

/// 插入的 `data`：纯插入在公共前后缀裁剪后天然只剩一个区间，因此一个
/// `kind:"add"` 块、`hunks` = 1（本命令一次调用只有一处插入点）、
/// `deleted_lines` = 0，`start_line_after` 即插入点在新文中的行号。
/// 两侧任一超出 diff 源上限时与 `写入文件` 同样降级为只有计数。
fn build_insert_data(path: &str, before: &str, after: &str) -> Value {
    if diff_source_too_large(before, after) {
        return build_edit_data(
            path,
            "update",
            &LineDiff::default(),
            Some(DIFF_UNAVAILABLE_OVERSIZE),
        );
    }
    build_edit_data(path, "update", &diff_replaced(before, after), None)
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
mod tests {
    use super::*;

    #[test]
    fn command_requested_only_for_non_empty_command() {
        assert!(command_requested(&json!({"command": "view"})));
        assert!(!command_requested(&json!({"command": "   "})));
        assert!(!command_requested(&json!({"command": null})));
        assert!(!command_requested(&json!({})));
    }

    #[test]
    fn view_range_parser_rejects_non_pairs() {
        assert!(parse_view_range(&json!({})).unwrap().is_none());
        assert!(parse_view_range(&json!({"view_range": null}))
            .unwrap()
            .is_none());
        assert!(parse_view_range(&json!({"view_range": [11, 12]}))
            .unwrap()
            .is_some());
        assert!(parse_view_range(&json!({"view_range": [11]})).is_err());
        assert!(parse_view_range(&json!({"view_range": [11, "x"]})).is_err());
    }

    #[test]
    fn file_view_numbers_lines_and_applies_range() {
        let rendered = format_file_view("f.txt", "alpha\nbeta\ngamma\n", None).unwrap();
        assert!(rendered.contains("     1  alpha"));
        assert!(rendered.contains("     3  gamma"));
        let ranged = format_file_view("f.txt", "alpha\nbeta\ngamma\n", Some(vec![2, 3])).unwrap();
        assert!(ranged.contains("with view_range=[2, 3]"));
        assert!(ranged.contains("     2  beta"));
        assert!(!ranged.contains("alpha"));
    }

    #[test]
    fn file_view_uses_actual_lines_and_bounds_long_line_rendering() {
        let rendered = format_file_view("sample.txt", "alpha\r\nbeta\r\n", None).unwrap();
        assert!(rendered.contains("total of 2 lines"));
        assert!(!rendered.contains('\r'));
        assert!(format_file_view("sample.txt", "alpha\n", Some(vec![2, -1])).is_err());
        assert!(format_file_view("sample.txt", "", None)
            .unwrap()
            .contains("total of 0 lines"));
        let long = "文".repeat(MAX_OUTPUT_CHARS * 4);
        let clipped = format_file_view("sample.txt", &long, None).unwrap();
        assert!(clipped.ends_with(TRUNCATED_MESSAGE));
        assert_eq!(
            clipped.chars().count(),
            MAX_OUTPUT_CHARS + TRUNCATED_MESSAGE.chars().count()
        );
    }

    #[test]
    fn truncation_appends_marker() {
        let out = maybe_truncate("0123456789", 4);
        assert!(out.starts_with("0123"));
        assert!(out.contains("<response clipped>"));
    }

    #[test]
    fn str_replace_data_carries_the_diff_contract() {
        let before = "alpha\nbeta\ngamma\n";
        let after = "alpha\nBETA\ngamma\n";
        let (diff, unavailable) = super::super::line_diff::diff_literal_replaces(
            before,
            after,
            &[6],
            "beta".len(),
            "BETA",
        );
        assert!(unavailable.is_none());
        let data = build_edit_data("f.txt", "update", &diff, unavailable);
        let mut keys: Vec<&str> = data
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
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
                "path",
                "updated",
            ]
        );
        assert_eq!(data["files"][0]["action"], json!("update"));
        assert_eq!(data["added_lines"], json!(1));
        assert_eq!(data["deleted_lines"], json!(1));
        assert_eq!(data["hunks_applied"], json!(1));
        let block = &data["files"][0]["diff_blocks"][0];
        assert_eq!(block["header"], json!("@@ -2,1 +2,1 @@"));
        assert_eq!(block["lines"][0]["kind"], json!("delete"));
        assert_eq!(block["lines"][0]["old_line"], json!(2));
        assert!(block["lines"][0]["new_line"].is_null());
        assert_eq!(block["lines"][0]["text"], json!("beta"));
        assert_eq!(block["lines"][1]["kind"], json!("add"));
        assert_eq!(block["lines"][1]["new_line"], json!(2));
        assert_eq!(block["lines"][1]["text"], json!("BETA"));
    }

    use crate::storage::SqliteStorage;
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn sorted_keys(value: &Value) -> Vec<String> {
        let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
        keys.sort();
        keys
    }

    /// 真实工作区夹具：直接跑 `run_editor` 分派，覆盖写盘与 diff 组装全链路。
    fn editor_workspace(root: &Path) -> Arc<WorkspaceManager> {
        let storage = Arc::new(SqliteStorage::new(
            root.join("state.sqlite3").to_string_lossy().to_string(),
        ));
        Arc::new(WorkspaceManager::new(
            root.join("workspaces").to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        ))
    }

    fn run(ws: &Arc<WorkspaceManager>, command: &str, args: &Value) -> Value {
        debug_assert_eq!(args["command"].as_str(), Some(command));
        run_editor(
            Arc::clone(ws),
            "tester".to_string(),
            Vec::new(),
            command,
            args.get("file_path").and_then(Value::as_str).unwrap_or(""),
            args,
        )
        .expect("editor dispatch")
        .value
    }

    fn create_args(path: &str, text: &str) -> Value {
        json!({ "command": "create", "file_path": path, "file_text": text })
    }

    #[test]
    fn create_emits_one_all_add_block_with_new_side_numbering() {
        let dir = tempdir().expect("tempdir");
        let ws = editor_workspace(dir.path());
        let result = run(
            &ws,
            "create",
            &create_args("notes.txt", "one\ntwo\nthree\n"),
        );
        assert_eq!(result["action"], json!("create"));
        assert_eq!(result["ok"], json!(true));
        let data = &result["data"];
        assert_eq!(data["path"], json!("notes.txt"));
        assert_eq!(data["added_lines"], json!(3));
        assert_eq!(data["deleted_lines"], json!(0));
        assert_eq!(data["changed_files"], json!(1));
        assert_eq!(data["added"], json!(1));
        assert_eq!(data["updated"], json!(0));
        assert_eq!(data["deleted"], json!(0));
        assert_eq!(data["moved"], json!(0));
        assert_eq!(data["hunks_applied"], json!(1));
        assert_eq!(data["diff_lines_omitted"], json!(0));
        assert!(data.get("diff_unavailable").is_none());
        let file = &data["files"][0];
        assert_eq!(file["action"], json!("add"));
        assert_eq!(file["path"], json!("notes.txt"));
        assert!(file["to_path"].is_null());
        assert_eq!(file["hunks"], json!(1));
        assert_eq!(file["diff_blocks"].as_array().expect("blocks").len(), 1);
        let block = &file["diff_blocks"][0];
        assert_eq!(block["header"], json!("new file"));
        assert_eq!(block["start_line_before"], json!(0));
        assert_eq!(block["end_line_before"], json!(0));
        assert_eq!(block["start_line_after"], json!(1));
        assert_eq!(block["end_line_after"], json!(3));
        let rows = block["lines"].as_array().expect("rows");
        assert_eq!(rows.len(), 3);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["kind"], json!("add"));
            assert!(row["old_line"].is_null());
            assert_eq!(row["new_line"], json!(index + 1));
        }
        assert_eq!(rows[0]["text"], json!("one"));
        assert_eq!(rows[2]["text"], json!("three"));

        // 落盘内容不变：view 仍能看到整份文件。
        let viewed = run(
            &ws,
            "view",
            &json!({ "command": "view", "file_path": "notes.txt" }),
        );
        assert!(viewed["data"]["content"]
            .as_str()
            .expect("content")
            .contains("     1  one"));
    }

    #[test]
    fn create_refuses_existing_path_without_claiming_a_diff() {
        let dir = tempdir().expect("tempdir");
        let ws = editor_workspace(dir.path());
        run(&ws, "create", &create_args("notes.txt", "a\n"));
        let again = run(&ws, "create", &create_args("notes.txt", "b\n"));
        assert_eq!(again["ok"], json!(false));
        assert_eq!(again["error_meta"]["code"], json!("TOOL_SRE_FILE_EXISTS"));
        assert!(again["data"].get("files").is_none());
    }

    #[test]
    fn insert_emits_add_only_block_at_the_insertion_point() {
        let dir = tempdir().expect("tempdir");
        let ws = editor_workspace(dir.path());
        run(
            &ws,
            "create",
            &create_args("notes.txt", "alpha\nbeta\ngamma\n"),
        );
        let result = run(
            &ws,
            "insert",
            &json!({
                "command": "insert",
                "file_path": "notes.txt",
                "insert_line": 2,
                "new_string": "X\nY",
            }),
        );
        assert_eq!(result["action"], json!("insert"));
        let data = &result["data"];
        assert_eq!(data["path"], json!("notes.txt"));
        assert_eq!(data["added_lines"], json!(2));
        assert_eq!(data["deleted_lines"], json!(0));
        assert_eq!(data["updated"], json!(1));
        assert_eq!(data["added"], json!(0));
        assert_eq!(data["hunks_applied"], json!(1));
        assert_eq!(data["changed_files"], json!(1));
        let file = &data["files"][0];
        assert_eq!(file["action"], json!("update"));
        assert_eq!(file["hunks"], json!(1));
        assert_eq!(file["deleted_lines"], json!(0));
        let block = &file["diff_blocks"][0];
        assert_eq!(file["diff_blocks"].as_array().expect("blocks").len(), 1);
        assert_eq!(block["header"], json!("@@ -3,0 +3,2 @@"));
        assert_eq!(block["start_line_after"], json!(3));
        assert_eq!(block["end_line_after"], json!(4));
        let rows = block["lines"].as_array().expect("rows");
        assert_eq!(rows.len(), 2);
        for row in rows.iter() {
            assert_eq!(row["kind"], json!("add"));
            assert!(row["old_line"].is_null());
        }
        assert_eq!(rows[0]["new_line"], json!(3));
        assert_eq!(rows[0]["text"], json!("X"));
        assert_eq!(rows[1]["new_line"], json!(4));
        assert_eq!(rows[1]["text"], json!("Y"));

        let viewed = run(
            &ws,
            "view",
            &json!({ "command": "view", "file_path": "notes.txt" }),
        );
        let content = viewed["data"]["content"].as_str().expect("content");
        assert!(content.contains("     3  X"), "{content}");
        assert!(content.contains("     5  gamma"), "{content}");
    }

    /// `create` 不允许覆盖既有路径（见 `create_refuses_existing_path_*`），
    /// 覆盖写语义属于 `写入文件`；这里钉住家族里 `update` 动作两侧编号的形态，
    /// 保证若日后放开覆盖，渲染侧契约不需要再改。
    #[test]
    fn update_action_numbers_both_sides_for_an_overwrite_shape() {
        let data = build_edit_data(
            "notes.txt",
            "update",
            &diff_replaced("alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\nDELTA\n"),
            None,
        );
        assert_eq!(data["updated"], json!(1));
        assert_eq!(data["added"], json!(0));
        assert_eq!(data["deleted_lines"], json!(2));
        assert_eq!(data["added_lines"], json!(3));
        let file = &data["files"][0];
        assert_eq!(file["action"], json!("update"));
        let rows = file["diff_blocks"][0]["lines"].as_array().expect("rows");
        let deletes: Vec<&Value> = rows
            .iter()
            .filter(|row| row["kind"] == json!("delete"))
            .collect();
        let adds: Vec<&Value> = rows
            .iter()
            .filter(|row| row["kind"] == json!("add"))
            .collect();
        assert_eq!(deletes.len(), 2);
        assert_eq!(adds.len(), 3);
        for (index, row) in deletes.iter().enumerate() {
            assert_eq!(row["old_line"], json!(index + 2));
            assert!(row["new_line"].is_null());
        }
        for (index, row) in adds.iter().enumerate() {
            assert_eq!(row["new_line"], json!(index + 2));
            assert!(row["old_line"].is_null());
        }
    }

    #[test]
    fn create_and_insert_data_pin_the_patch_family_key_set() {
        let dir = tempdir().expect("tempdir");
        let ws = editor_workspace(dir.path());
        let created = run(&ws, "create", &create_args("a.txt", "one\n"));
        let expected = vec![
            "added",
            "added_lines",
            "changed_files",
            "deleted",
            "deleted_lines",
            "diff_lines_omitted",
            "files",
            "hunks_applied",
            "moved",
            "path",
            "updated",
        ];
        assert_eq!(sorted_keys(&created["data"]), expected);
        let inserted = run(
            &ws,
            "insert",
            &json!({
                "command": "insert",
                "file_path": "a.txt",
                "insert_line": 1,
                "new_string": "two",
            }),
        );
        assert_eq!(sorted_keys(&inserted["data"]), expected);
        let block = &inserted["data"]["files"][0]["diff_blocks"][0];
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
        assert_eq!(
            sorted_keys(&block["lines"][0]),
            vec!["kind", "new_line", "old_line", "text"]
        );
        for row in block["lines"].as_array().expect("rows") {
            let kind = row["kind"].as_str().expect("kind");
            assert!(matches!(kind, "meta" | "add" | "delete"), "{kind}");
            let text = row["text"].as_str().expect("text");
            assert!(!text.starts_with('+') && !text.starts_with('-'), "{text}");
        }
    }

    #[test]
    fn insert_degrades_to_counts_only_for_oversized_sources() {
        use super::super::line_diff::MAX_DIFF_SOURCE_BYTES;
        let dir = tempdir().expect("tempdir");
        let ws = editor_workspace(dir.path());
        let big = "x\n".repeat(MAX_DIFF_SOURCE_BYTES / 2 + 1);
        run(&ws, "create", &create_args("big.txt", &big));
        let result = run(
            &ws,
            "insert",
            &json!({
                "command": "insert",
                "file_path": "big.txt",
                "insert_line": 1,
                "new_string": "y",
            }),
        );
        let data = &result["data"];
        assert_eq!(data["diff_unavailable"], json!("oversize"));
        assert_eq!(data["files"][0]["action"], json!("update"));
        assert_eq!(data["files"][0]["added_lines"], json!(0));
        assert!(data["files"][0]["diff_blocks"]
            .as_array()
            .expect("blocks")
            .is_empty());
    }
}
