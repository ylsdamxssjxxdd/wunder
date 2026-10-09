//! 基础编辑工具：「文本编辑」。
//!
//! 由原 `编辑` 与 `str_replace_editor` 合并而来：字面替换为主，另有
//! 补丁（`input`）与子命令（`command`）两个兼容形态，见 [`text_edit`]。
//!
//! 语义对齐 dsh 的 `edit`：在既有 UTF-8 文本文件里做**字面文本替换**。
//! `old_string` 必须唯一匹配，除非显式 `replace_all=true`；`new_string`
//! 允许为空以删除所选文本。文件先经能力 seam（`FsCapability`）读取，
//! 再由 `atomic_write_text` 原子落盘；工作区内路径沿用 workspace 记账。

use super::capabilities::{local_fs, FsCapability};
use super::context::collect_allow_roots;
use super::{
    build_model_tool_success,
    command_options::parse_dry_run,
    execute_in_sandbox, recover_tool_args_value, resolve_tool_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    touch_lsp_file, ToolContext,
};
use crate::core::blocking;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) const TOOL_EDIT: &str = "文本编辑";

/// 判断参数是否以补丁（patch）形态调用，兼容旧 `应用补丁` 的 `input`/`patch`。
pub(crate) fn has_patch_input(args: &Value) -> bool {
    ["input", "patch"].iter().any(|key| {
        args.get(*key)
            .and_then(Value::as_str)
            .map(|raw| !raw.trim().is_empty())
            .unwrap_or(false)
    })
}

/// 可区分的编辑失败原因，便于给出可复现的恢复提示。
#[derive(Debug)]
enum EditFailure {
    NotFound,
    NoMatch,
    NotUnique(usize),
}

impl std::fmt::Display for EditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditFailure::NotFound => write!(f, "目标文件不存在"),
            EditFailure::NoMatch => write!(f, "old_string 未匹配"),
            EditFailure::NotUnique(count) => write!(f, "old_string 匹配到 {count} 处"),
        }
    }
}

impl std::error::Error for EditFailure {}

struct EditOutcome {
    target: PathBuf,
    occurrences: usize,
    bytes_before: usize,
    bytes_after: usize,
}

/// 模型侧参数的最小校验，返回 `Some(failure)` 表示调用不合法。
pub(crate) fn validate_edit_args(args: &Value) -> Option<Value> {
    let file_path = args
        .get("file_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(file_path) = file_path else {
        return Some(build_failed_tool_result(
            "缺少 file_path",
            json!({"file_path_required": true}),
            ToolErrorMeta::new(
                "TOOL_EDIT_PATH_REQUIRED",
                Some("请提供非空字符串 file_path（也接受 path）。".to_string()),
                false,
                None,
            ),
            false,
        ));
    };

    let old_string = args
        .get("old_string")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    if old_string.is_none() {
        return Some(build_failed_tool_result(
            "缺少 old_string",
            json!({"file_path": file_path, "old_string_required": true}),
            ToolErrorMeta::new(
                "TOOL_EDIT_OLD_STRING_REQUIRED",
                Some("old_string 必填且不能为空；若要删除内容，请给出被删除的原文，new_string 留空。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    if !args.get("new_string").is_some_and(Value::is_string) {
        return Some(build_failed_tool_result(
            "缺少 new_string",
            json!({"file_path": file_path, "new_string_required": true}),
            ToolErrorMeta::new(
                "TOOL_EDIT_NEW_STRING_REQUIRED",
                Some("请提供字符串 new_string；空字符串表示删除匹配文本。".to_string()),
                false,
                None,
            ),
            false,
        ));
    }

    None
}

/// 统一入口：`文本编辑`。
///
/// 三种形态按优先级分派：
/// 1. 携带非空 `command` → 子命令编辑（`view`/`create`/`str_replace`/`insert`）；
/// 2. 携带补丁输入（非空 `input`/`patch`）→ 补丁模式（原 `应用补丁`）；
/// 3. 其余 → 字面文本替换（dsh `edit` 语义）。
pub(crate) async fn text_edit(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if super::str_replace_editor_tool::command_requested(args) {
        return super::str_replace_editor_tool::str_replace_editor(context, args).await;
    }
    let args = recover_tool_args_value(args);
    if has_patch_input(&args) {
        return super::apply_patch_tool::apply_patch(context, &args).await;
    }
    edit_file(context, &args).await
}

pub(crate) async fn edit_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    if let Some(failure) = validate_edit_args(&args) {
        return Ok(failure);
    }
    if let Some(result) = execute_in_sandbox(context, TOOL_EDIT, &args).await {
        if !parse_dry_run(&args) {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }

    let file_path = args
        .get("file_path")
        .and_then(Value::as_str)
        .expect("edit arguments were validated")
        .trim()
        .to_string();
    let old_string = args
        .get("old_string")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let new_string = args
        .get("new_string")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let replace_all = args
        .get("replace_all")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let dry_run = parse_dry_run(&args);

    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let allow_roots = collect_allow_roots(context);
    let path_for_edit = file_path.clone();
    let old_for_edit = old_string.clone();
    let new_for_edit = new_string.clone();
    let outcome = blocking::run_fs("tools.file.edit", move || {
        let target = resolve_tool_path(workspace.as_ref(), &user_id, &path_for_edit, &allow_roots)?;
        if target.is_dir() {
            return Err(anyhow!("target path is a directory"));
        }
        if !target.exists() {
            return Err(anyhow::Error::new(EditFailure::NotFound));
        }
        let fs_cap = local_fs();
        let content = fs_cap.read_text(&target)?;
        let occurrences = content.matches(&old_for_edit).count();
        if occurrences == 0 {
            return Err(anyhow::Error::new(EditFailure::NoMatch));
        }
        if occurrences > 1 && !replace_all {
            return Err(anyhow::Error::new(EditFailure::NotUnique(occurrences)));
        }
        let updated = if replace_all {
            content.replace(&old_for_edit, &new_for_edit)
        } else {
            content.replacen(&old_for_edit, &new_for_edit, 1)
        };
        if !dry_run {
            let workspace_root = workspace.workspace_root(&user_id);
            let default_target = workspace.resolve_path(&user_id, &path_for_edit)?;
            if is_within_root(&workspace_root, &target)
                && normalize_path_for_compare(&normalize_target_path(&target))
                    == normalize_path_for_compare(&normalize_target_path(&default_target))
            {
                workspace.write_file(&user_id, &path_for_edit, &updated, true)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs_cap.create_dir_all(parent)?;
                }
                fs_cap.write_text(&target, &updated)?;
            }
        }
        Ok::<EditOutcome, anyhow::Error>(EditOutcome {
            target,
            occurrences,
            bytes_before: content.len(),
            bytes_after: updated.len(),
        })
    })
    .await;

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(err) => {
            if let Some(failure) = err.downcast_ref::<EditFailure>() {
                let (message, code, hint) = match failure {
                    EditFailure::NotFound => (
                        format!("文本编辑失败：文件不存在 {file_path}"),
                        "TOOL_EDIT_NOT_FOUND",
                        "确认路径正确；新建文件请改用写入文件。",
                    ),
                    EditFailure::NoMatch => (
                        "文本编辑失败：old_string 未在文件中匹配".to_string(),
                        "TOOL_EDIT_NO_MATCH",
                        "确保 old_string 与文件内容逐字一致（包含空白与缩进）。",
                    ),
                    EditFailure::NotUnique(count) => (
                        format!("文本编辑失败：old_string 匹配到 {count} 处，无法唯一替换"),
                        "TOOL_EDIT_NOT_UNIQUE",
                        "补充上下文使匹配唯一，或设置 replace_all=true 全部替换。",
                    ),
                };
                return Ok(build_failed_tool_result(
                    message,
                    json!({
                        "path": file_path,
                        "dry_run": dry_run,
                    }),
                    ToolErrorMeta::new(code, Some(hint.to_string()), true, Some(200)),
                    false,
                ));
            }
            return Ok(build_failed_tool_result(
                format!("文本编辑失败：{err}"),
                json!({
                    "path": file_path,
                    "dry_run": dry_run,
                }),
                ToolErrorMeta::new(
                    "TOOL_EDIT_FAILED",
                    Some("请确认路径权限与文件状态后重试。".to_string()),
                    true,
                    Some(200),
                ),
                false,
            ));
        }
    };

    if !dry_run {
        context.workspace.mark_tree_dirty(context.workspace_id);
    }
    let lsp_info = if dry_run {
        Value::Null
    } else {
        touch_lsp_file(context, &outcome.target, true).await
    };

    Ok(build_model_tool_success(
        "edit",
        if dry_run { "dry_run" } else { "completed" },
        if dry_run {
            format!("Validated edit against {file_path} without writing content.")
        } else {
            format!(
                "Edited {file_path} ({} replacement(s)).",
                outcome.occurrences
            )
        },
        json!({
            "path": file_path,
            "replacements": outcome.occurrences,
            "replace_all": replace_all,
            "bytes_before": outcome.bytes_before,
            "bytes_after": outcome.bytes_after,
            "dry_run": dry_run,
            "lsp": lsp_info,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_edit_routes_by_shape() {
        use super::super::str_replace_editor_tool::command_requested;
        assert!(command_requested(&json!({"command": "view"})));
        assert!(!command_requested(&json!({"command": "   "})));
        assert!(!command_requested(&json!({"command": null})));
        assert!(!command_requested(&json!({})));
        assert!(has_patch_input(&json!({"input": "*** Begin Patch"})));
        assert!(!has_patch_input(&json!({"input": "   "})));
        assert!(!has_patch_input(&json!({})));
    }

    #[test]
    fn validates_required_params() {
        assert!(validate_edit_args(&json!({})).is_some());
        assert!(validate_edit_args(&json!({"file_path": "a.txt"})).is_some());
        assert!(validate_edit_args(&json!({"file_path": "a.txt", "old_string": ""})).is_some());
        assert!(validate_edit_args(&json!({"file_path": "a.txt", "old_string": "x"})).is_some());
    }

    #[test]
    fn accepts_empty_new_string_to_delete() {
        let failure = validate_edit_args(&json!({
            "file_path": "a.txt",
            "old_string": "x",
            "new_string": ""
        }));
        assert!(failure.is_none(), "空 new_string 应当合法（表示删除）");
    }
}
