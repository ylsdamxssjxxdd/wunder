//! dsh 风格的多命令编辑器：`str_replace_editor`。
//!
//! 对齐 dsh 的 `str_replace_editor`：一个带子命令的编辑器。
//! - `view`：文件按 `cat -n` 风格带行号渲染（六位右对齐 + 两空格），目录列出
//!   最多两层、过滤隐藏项与 `node_modules`/`__pycache__`；
//! - `create`：仅在目标不存在时新建；
//! - `str_replace`：字面文本替换，要求 `old_str` 唯一且逐字出现；
//! - `insert`：把 `new_str` 插入到第 `insert_line` 行之后。
//!
//! 输出超过 `MAX_OUTPUT_CHARS` 时按字符截断并追加 `<response clipped>` 说明。
//! 路径沿用 workspace 记账，工作区外的允许根走能力 seam。

use super::capabilities::{local_fs, FsCapability};
use super::context::collect_allow_roots;
use super::{
    build_model_tool_success, recover_tool_args_value, resolve_tool_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    ToolContext,
};
use crate::core::blocking;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use crate::workspace::WorkspaceManager;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// Canonical tool name.
pub(crate) const TOOL_STR_REPLACE_EDITOR: &str = "str_replace_editor";

/// Maximum returned characters before clipping (dsh default).
const MAX_OUTPUT_CHARS: usize = 16_000;

const TRUNCATED_MESSAGE: &str = "<response clipped><NOTE>To save on context only part of this file has been shown to you. You should retry this tool after you have searched inside the file with `grep -n` in order to find the line numbers of what you are looking for.</NOTE>";

const VALID_COMMANDS: [&str; 4] = ["view", "create", "str_replace", "insert"];

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
    let args = recover_tool_args_value(args);
    if let Some(result) = super::execute_in_sandbox(context, TOOL_STR_REPLACE_EDITOR, &args).await {
        return Ok(result);
    }

    let command = args
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(command) = command else {
        return Ok(missing_param("command", "str_replace_editor"));
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
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(path) = path else {
        return Ok(editor_failure(
            "Parameter `path` must be a non-empty string.",
            json!({ "path_required": true }),
            "TOOL_SRE_PATH_REQUIRED",
            "提供非空字符串 path。",
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
            &format!("str_replace_editor failed: {err}"),
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
        "str_replace" => replace_command(ws, &user_id, &target, path, args),
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
    write_target(ws, user_id, target, path, &file_text)?;
    Ok(ok(
        success(
            "create",
            &format!("New file created successfully at: {path}"),
            json!({ "path": path }),
        ),
        true,
    ))
}

fn replace_command(
    ws: &WorkspaceManager,
    user_id: &str,
    target: &Path,
    path: &str,
    args: &Value,
) -> Result<EditorOutcome> {
    if matches!(args.get("new_str"), Some(Value::Null)) {
        return Ok(fail(editor_failure(
            "Parameter `new_str` must be omitted or contain a string for command: str_replace",
            json!({ "path": path }),
            "TOOL_SRE_NEW_STR_NULL",
            "删除匹配文本请省略 new_str 或传空字符串。",
        )));
    }
    let old_str = match args.get("old_str").and_then(Value::as_str) {
        Some(value) if !value.is_empty() => value.to_string(),
        Some(_) => {
            return Ok(fail(editor_failure(
                "Parameter `old_str` is empty for command: str_replace",
                json!({ "path": path }),
                "TOOL_SRE_OLD_STR_EMPTY",
                "old_str 必填且不能为空。",
            )));
        }
        None => return Ok(fail(missing_param("old_str", "str_replace"))),
    };
    let new_str = args
        .get("new_str")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !target.exists() {
        return Ok(fail(not_found(path)));
    }
    if target.is_dir() {
        return Ok(fail(not_regular_file(path)));
    }
    let fs_cap = local_fs();
    let before = fs_cap.read_text(target)?;
    let offsets = match_offsets(&before, &old_str);
    let Some(&offset) = offsets.first() else {
        return Ok(fail(editor_failure(
            &format!(
                "No replacement was performed, old_str `{old_str}` did not appear verbatim in {path}."
            ),
            json!({ "path": path }),
            "FS_EDIT_NOT_FOUND",
            "确保 old_str 与文件内容逐字一致（含空白与缩进）。",
        )));
    };
    if offsets.len() > 1 {
        let lines = line_numbers_at(&before, &offsets);
        let joined = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Ok(fail(editor_failure(
            &format!(
                "No replacement was performed. Multiple occurrences of old_str `{old_str}` in lines [{joined}]. Please ensure it is unique"
            ),
            json!({ "path": path }),
            "FS_AMBIGUOUS_EDIT",
            "补充上下文使 old_str 唯一。",
        )));
    }
    let updated = format!(
        "{}{}{}",
        &before[..offset],
        new_str,
        &before[offset + old_str.len()..]
    );
    write_target(ws, user_id, target, path, &updated)?;
    Ok(ok(
        success(
            "str_replace",
            &format!("The file {path} has been edited successfully."),
            json!({ "path": path }),
        ),
        true,
    ))
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
    let new_str = match args.get("new_str").and_then(Value::as_str) {
        Some(value) => value.to_string(),
        None => return Ok(fail(missing_param("new_str", "insert"))),
    };
    if !target.exists() {
        return Ok(fail(not_found(path)));
    }
    if target.is_dir() {
        return Ok(fail(not_regular_file(path)));
    }
    let fs_cap = local_fs();
    let before = fs_cap.read_text(target)?;
    let lines: Vec<&str> = before.split('\n').collect();
    if insert_line < 0 || insert_line > lines.len() as i64 {
        return Ok(fail(editor_failure(
            &format!(
                "Invalid `insert_line` parameter: {insert_line}. It should be within the range of lines of the file: [0, {}]",
                lines.len()
            ),
            json!({ "path": path }),
            "TOOL_SRE_INVALID_INSERT_LINE",
            "insert_line 取值需落在 [0, 总行数]。",
        )));
    }
    let index = insert_line as usize;
    let mut after: Vec<String> = Vec::with_capacity(lines.len() + 1);
    after.extend(lines[..index].iter().map(|line| (*line).to_string()));
    after.extend(new_str.split('\n').map(|line| line.to_string()));
    after.extend(lines[index..].iter().map(|line| (*line).to_string()));
    let updated = after.join("\n");
    write_target(ws, user_id, target, path, &updated)?;
    Ok(ok(
        success(
            "insert",
            &format!("The file {path} has been edited successfully."),
            json!({ "path": path }),
        ),
        true,
    ))
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
    let all_lines: Vec<&str> = content.split('\n').collect();
    let total = all_lines.len();
    let mut initial_line: i64 = 1;
    let mut prompt = format!(
        "Here's the content of {path} with line numbers (which has a total of {total} lines)"
    );
    let mut lines: Vec<&str> = all_lines.clone();
    if let Some(range) = view_range {
        if range.len() != 2 {
            return Err("Invalid `view_range`. It should be a list of two integers.".to_string());
        }
        let start = range[0];
        let end = range[1];
        initial_line = start;
        if start < 1 || start > total as i64 {
            return Err(format!(
                "Invalid `view_range`: [{start}, {end}]. Its first element `{start}` should be within the range of lines of the file: [1, {total}]"
            ));
        }
        if end > total as i64 {
            return Err(format!(
                "Invalid `view_range`: [{start}, {end}]. Its second element `{end}` should be smaller than the number of lines in the file: `{total}`"
            ));
        }
        if end != -1 && end < start {
            return Err(format!(
                "Invalid `view_range`: [{start}, {end}]. Its second element `{end}` should be larger or equal than its first `{start}`"
            ));
        }
        let begin = (start - 1) as usize;
        lines = if end == -1 {
            all_lines[begin..].to_vec()
        } else {
            all_lines[begin..end as usize].to_vec()
        };
        prompt += &format!(" with view_range=[{start}, {end}]");
    }
    let numbered = lines
        .iter()
        .enumerate()
        .map(|(index, line)| format!("{:>6}  {}", initial_line + index as i64, line))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(maybe_truncate(
        &format!("{prompt}:\n{numbered}\n"),
        MAX_OUTPUT_CHARS,
    ))
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

/// Byte offsets of every occurrence of `search` in `content`.
fn match_offsets(content: &str, search: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    if search.is_empty() {
        return offsets;
    }
    let mut from = 0usize;
    while let Some(found) = content[from..].find(search) {
        let offset = from + found;
        offsets.push(offset);
        from = offset + search.len();
        if from >= content.len() {
            break;
        }
    }
    offsets
}

/// 1-based line numbers for the given byte offsets.
fn line_numbers_at(content: &str, offsets: &[usize]) -> Vec<usize> {
    let bytes = content.as_bytes();
    let mut line = 1usize;
    let mut cursor = 0usize;
    offsets
        .iter()
        .map(|&offset| {
            while cursor < offset && cursor < bytes.len() {
                if bytes[cursor] == b'\n' {
                    line += 1;
                }
                cursor += 1;
            }
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn match_offsets_finds_every_occurrence() {
        assert_eq!(match_offsets("aXbXc", "X"), vec![1, 3]);
        assert!(match_offsets("abc", "z").is_empty());
        assert!(match_offsets("abc", "").is_empty());
    }

    #[test]
    fn truncation_appends_marker() {
        let out = maybe_truncate("0123456789", 4);
        assert!(out.starts_with("0123"));
        assert!(out.contains("<response clipped>"));
    }
}
