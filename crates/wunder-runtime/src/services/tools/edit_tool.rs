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
use super::line_diff::{diff_literal_replaces, insert_diff_fields, LineDiff};
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
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub(crate) const TOOL_EDIT: &str = "文本编辑";

/// 采集的命中数上限：diff 的行数统计只覆盖这些命中，命中总数由
/// `replacements`/`hunks` 给出；块数上限另由 `line_diff` 收口。
const MAX_DIFF_OFFSETS: usize = 200;

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
    /// 字面替换的真实 diff；`unavailable` 有值时只带计数不带块。
    diff: LineDiff,
    unavailable: Option<&'static str>,
}

/// 字面替换路由的 diff 构造：有界采样命中起点，块上限与有界性守卫都在
/// `line_diff` 里收口；命中数被采样上限截断时 `hunks` 仍报真实替换次数。
fn build_replace_diff(
    before: &str,
    after: &str,
    offsets: &[usize],
    occurrences: usize,
    needle_len: usize,
    replacement: &str,
) -> (LineDiff, Option<&'static str>) {
    let (diff, unavailable) =
        diff_literal_replaces(before, after, offsets, needle_len, replacement);
    if unavailable.is_none() && occurrences > offsets.len() {
        return (diff.with_hunks(occurrences), None);
    }
    (diff, unavailable)
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
        // diff 只需要有界个命中起点；命中总数仍由 matches().count() 给出。
        let offsets: Vec<usize> = if replace_all {
            content
                .match_indices(&old_for_edit)
                .map(|(index, _)| index)
                .take(MAX_DIFF_OFFSETS)
                .collect()
        } else {
            content.find(&old_for_edit).into_iter().collect()
        };
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
        let (diff, unavailable) = build_replace_diff(
            &content,
            &updated,
            &offsets,
            occurrences,
            old_for_edit.len(),
            &new_for_edit,
        );
        Ok::<EditOutcome, anyhow::Error>(EditOutcome {
            target,
            occurrences,
            bytes_before: content.len(),
            bytes_after: updated.len(),
            diff,
            unavailable,
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

    let data = build_edit_data(
        &file_path,
        outcome.occurrences,
        replace_all,
        outcome.bytes_before,
        outcome.bytes_after,
        dry_run,
        lsp_info,
        &outcome.diff,
        outcome.unavailable,
    );

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
        data,
    ))
}

/// 组装字面替换结果 `data`：既有键保持不变，补丁家族的 diff 键一并给出。
fn build_edit_data(
    path: &str,
    occurrences: usize,
    replace_all: bool,
    bytes_before: usize,
    bytes_after: usize,
    dry_run: bool,
    lsp: Value,
    diff: &LineDiff,
    unavailable: Option<&'static str>,
) -> Value {
    let mut diff_fields = Map::<String, Value>::new();
    insert_diff_fields(&mut diff_fields, "update", path, diff);
    if let Some(reason) = unavailable {
        diff_fields.insert("diff_unavailable".to_string(), json!(reason));
    }
    let mut data = json!({
        "path": path,
        "replacements": occurrences,
        "replace_all": replace_all,
        "bytes_before": bytes_before,
        "bytes_after": bytes_after,
        "dry_run": dry_run,
        "lsp": lsp,
    });
    if let Some(map) = data.as_object_mut() {
        map.extend(diff_fields);
    }
    data
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

    fn replace_all_offsets(count: usize) -> Vec<usize> {
        (0..count).map(|index| index * 2).collect()
    }

    /// 断言全部走生产路径产出的 `data`，不触碰 `line_diff` 内部字段。
    fn edit_data_for(
        before: &str,
        after: &str,
        offsets: &[usize],
        needle: &str,
        new: &str,
    ) -> Value {
        let occurrences = offsets.len().max(1);
        let (diff, unavailable) =
            build_replace_diff(before, after, offsets, occurrences, needle.len(), new);
        build_edit_data(
            "a.txt",
            occurrences,
            offsets.len() > 1,
            before.len(),
            after.len(),
            false,
            Value::Null,
            &diff,
            unavailable,
        )
    }

    #[test]
    fn single_hit_edit_reports_line_numbers_on_both_sides() {
        let data = edit_data_for(
            "alpha\nbeta\ngamma\n",
            "alpha\nBETA\ngamma\n",
            &[6],
            "beta",
            "BETA",
        );
        assert_eq!(data["added_lines"], json!(1));
        assert_eq!(data["deleted_lines"], json!(1));
        assert_eq!(data["hunks_applied"], json!(1));
        assert_eq!(data["replacements"], json!(1));
        let file = &data["files"][0];
        assert_eq!(file["action"], json!("update"));
        assert_eq!(file["path"], json!("a.txt"));
        let block = &file["diff_blocks"][0];
        assert_eq!(block["header"], json!("@@ -2,1 +2,1 @@"));
        assert_eq!(block["lines"][0]["kind"], json!("delete"));
        assert_eq!(block["lines"][0]["old_line"], json!(2));
        assert!(block["lines"][0]["new_line"].is_null());
        assert_eq!(block["lines"][0]["text"], json!("beta"));
        assert_eq!(block["lines"][1]["kind"], json!("add"));
        assert_eq!(block["lines"][1]["new_line"], json!(2));
        assert_eq!(block["lines"][1]["text"], json!("BETA"));
    }

    #[test]
    fn replace_all_emits_one_block_per_hit_and_honours_block_cap() {
        let before = "x\n".repeat(30);
        let after = "y\n".repeat(30);
        let data = edit_data_for(&before, &after, &replace_all_offsets(30), "x", "y");
        let blocks = data["files"][0]["diff_blocks"].as_array().expect("blocks");
        // 一个命中一个块，块数上限由 line_diff 收口为 12。
        assert_eq!(blocks.len(), 12);
        assert_eq!(blocks[11]["start_line_before"], json!(12));
        assert_eq!(data["hunks_applied"], json!(30));
        assert_eq!(data["added_lines"], json!(30));
        assert_eq!(data["deleted_lines"], json!(30));
        assert_eq!(data["diff_lines_omitted"], json!(36));
    }

    #[test]
    fn sampled_hits_keep_exact_hunk_count() {
        let before = "x\n".repeat(MAX_DIFF_OFFSETS + 5);
        let after = "y\n".repeat(MAX_DIFF_OFFSETS + 5);
        let offsets = replace_all_offsets(MAX_DIFF_OFFSETS);
        let (diff, unavailable) =
            build_replace_diff(&before, &after, &offsets, 205, "x".len(), "y");
        assert!(unavailable.is_none());
        let data = build_edit_data(
            "a.txt",
            205,
            true,
            before.len(),
            after.len(),
            false,
            Value::Null,
            &diff,
            unavailable,
        );
        assert_eq!(offsets.len(), MAX_DIFF_OFFSETS);
        // 采样截断只影响块数，不影响替换次数。
        assert_eq!(data["hunks_applied"], json!(205));
        assert_eq!(data["replacements"], json!(205));
        assert_eq!(data["files"][0]["hunks"], json!(205));
        assert_eq!(
            data["files"][0]["diff_blocks"]
                .as_array()
                .expect("blocks")
                .len(),
            12
        );
        assert_eq!(data["added_lines"], json!(MAX_DIFF_OFFSETS));
    }

    #[test]
    fn oversized_source_keeps_counts_only() {
        let before = "x\n".repeat(300_000);
        let offsets = [0usize];
        let (diff, unavailable) = build_replace_diff(&before, "y\n", &offsets, 1, "x".len(), "y");
        assert_eq!(unavailable, Some("oversize"));
        let data = build_edit_data(
            "a.txt",
            1,
            false,
            before.len(),
            2,
            false,
            Value::Null,
            &diff,
            unavailable,
        );
        assert_eq!(data["diff_unavailable"], json!("oversize"));
        assert_eq!(data["added_lines"], json!(0));
        assert!(data["files"][0]["diff_blocks"]
            .as_array()
            .expect("array")
            .is_empty());
    }

    #[test]
    fn edit_data_key_set_matches_the_contract() {
        let data = edit_data_for("a\nb\n", "a\nc\n", &[2], "b", "c");
        let map = data.as_object().expect("object");
        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "added",
                "added_lines",
                "bytes_after",
                "bytes_before",
                "changed_files",
                "deleted",
                "deleted_lines",
                "diff_lines_omitted",
                "dry_run",
                "files",
                "hunks_applied",
                "lsp",
                "moved",
                "path",
                "replace_all",
                "replacements",
                "updated",
            ]
        );
        let file = &data["files"][0];
        let mut file_keys: Vec<&str> = file
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        file_keys.sort_unstable();
        assert_eq!(
            file_keys,
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
    }
}
