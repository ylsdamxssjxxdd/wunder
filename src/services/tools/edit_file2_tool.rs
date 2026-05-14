use super::command_options::parse_dry_run;
use super::tool_error::{build_failed_tool_result, ToolErrorMeta};
use super::{
    build_model_tool_success, collect_orchestration_aware_allow_roots, execute_in_sandbox,
    recover_tool_args_value, resolve_tool_path, touch_lsp_file, ToolContext,
};
use crate::core::atomic_write::atomic_write_text;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
enum EditAction {
    Replace,
    ReplaceBetween,
    InsertBefore,
    InsertAfter,
    Append,
    Prepend,
}

#[derive(Debug, Clone)]
struct EditInstruction {
    action: EditAction,
    old_text: Option<String>,
    new_text: String,
    start_marker: Option<String>,
    end_marker: Option<String>,
    anchor: Option<String>,
    replace_all: bool,
    expected_count: Option<usize>,
    unique: bool,
}

#[derive(Debug)]
struct EditInstructionOutcome {
    action: &'static str,
    changed: bool,
    matches: usize,
    inserted_bytes: usize,
}

#[derive(Debug)]
struct EditFile2Plan {
    path: String,
    instructions: Vec<EditInstruction>,
    dry_run: bool,
    ensure_newline: bool,
}

#[derive(Debug)]
struct EditFile2Outcome {
    target: PathBuf,
    existed: bool,
    previous_bytes: u64,
    new_bytes: usize,
    change_count: usize,
    instruction_results: Vec<Value>,
}

pub(crate) async fn edit_file2(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = recover_tool_args_value(args);
    if let Some(result) = execute_in_sandbox(context, "文本编辑", &args).await {
        if !parse_dry_run(&args) {
            context.workspace.mark_tree_dirty(context.workspace_id);
        }
        return Ok(result);
    }

    let plan = match parse_edit_file2_plan(&args) {
        Ok(plan) => plan,
        Err((error, meta)) => {
            return Ok(build_failed_tool_result(error, json!({}), meta, false));
        }
    };
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let path_for_write = plan.path.clone();
    let allow_roots = collect_orchestration_aware_allow_roots(context);
    let dry_run = plan.dry_run;
    let ensure_newline = plan.ensure_newline;
    let instructions = plan.instructions.clone();

    let outcome = tokio::task::spawn_blocking(move || {
        execute_edit_file2_plan(
            workspace.as_ref(),
            &user_id,
            &path_for_write,
            &allow_roots,
            &instructions,
            dry_run,
            ensure_newline,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()));

    let outcome = match outcome {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(err)) | Err(err) => {
            return Ok(build_failed_tool_result(
                format!("文本编辑失败：{err}"),
                json!({
                    "path": plan.path,
                    "dry_run": plan.dry_run,
                }),
                ToolErrorMeta::new(
                    "TOOL_EDIT2_FAILED",
                    Some(
                        "请先 read_file 读取最新文本，并确认 old_text 与文件内容完全一致；多处、条件或跨段替换请改用 programmatic_tool_call 写 Python 脚本。"
                            .to_string(),
                    ),
                    true,
                    Some(200),
                ),
                false,
            ));
        }
    };
    let lsp_info = if plan.dry_run {
        Value::Null
    } else {
        touch_lsp_file(context, &outcome.target, true).await
    };

    Ok(build_model_tool_success(
        "edit_file2",
        if plan.dry_run { "dry_run" } else { "completed" },
        if plan.dry_run {
            format!(
                "Validated {} edit steps for {} without writing.",
                outcome.change_count, plan.path
            )
        } else if outcome.existed {
            format!(
                "Updated file {} with {} edit steps.",
                plan.path, outcome.change_count
            )
        } else {
            format!(
                "Created file {} with {} edit steps.",
                plan.path, outcome.change_count
            )
        },
        json!({
            "path": plan.path,
            "dry_run": plan.dry_run,
            "ensure_newline": plan.ensure_newline,
            "existed": outcome.existed,
            "previous_bytes": outcome.previous_bytes,
            "bytes": outcome.new_bytes,
            "edit_count": outcome.change_count,
            "edits": outcome.instruction_results,
            "lsp": lsp_info
        }),
    ))
}

fn parse_edit_file2_plan(
    args: &Value,
) -> std::result::Result<EditFile2Plan, (String, ToolErrorMeta)> {
    let path = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if path.is_empty() {
        return Err((
            "缺少 path".to_string(),
            ToolErrorMeta::new(
                "TOOL_EDIT2_PATH_REQUIRED",
                Some("请提供要编辑的文本文件路径。".to_string()),
                false,
                None,
            ),
        ));
    }
    let instructions = if let Some(instruction) = parse_flat_replace_instruction(&args)? {
        vec![instruction]
    } else {
        let instructions_value = args
            .get("edits")
            .or_else(|| args.get("operations"))
            .or_else(|| args.get("steps"));
        let Some(instructions_value) = instructions_value else {
            return Err((
                "缺少 old_text/new_text".to_string(),
                ToolErrorMeta::new(
                    "TOOL_EDIT2_REPLACE_REQUIRED",
                    Some("请只提供 path、old_text、new_text。复杂多处或条件替换请改用 programmatic_tool_call 写 Python 脚本。".to_string()),
                    false,
                    None,
                ),
            ));
        };
        parse_edit_instructions(instructions_value)?
    };
    let dry_run = parse_dry_run(args);
    let ensure_newline = args
        .get("ensure_newline")
        .or_else(|| args.get("ensureNewline"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(EditFile2Plan {
        path,
        instructions,
        dry_run,
        ensure_newline,
    })
}

fn parse_flat_replace_instruction(
    args: &Value,
) -> std::result::Result<Option<EditInstruction>, (String, ToolErrorMeta)> {
    let has_flat_fields = args.get("old_text").is_some()
        || args.get("oldText").is_some()
        || args.get("find").is_some()
        || args.get("target").is_some()
        || args.get("new_text").is_some()
        || args.get("newText").is_some()
        || args.get("replacement").is_some();
    if !has_flat_fields {
        return Ok(None);
    }
    let old_text = args
        .get("old_text")
        .or_else(|| args.get("oldText"))
        .or_else(|| args.get("find"))
        .or_else(|| args.get("target"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if old_text.is_empty() {
        return Err((
            "缺少 old_text".to_string(),
            ToolErrorMeta::new(
                "TOOL_EDIT2_OLD_TEXT_REQUIRED",
                Some("请提供要被替换的精确原文 old_text。".to_string()),
                false,
                None,
            ),
        ));
    }
    let new_text_value = args
        .get("new_text")
        .or_else(|| args.get("newText"))
        .or_else(|| args.get("replacement"));
    let Some(new_text) = new_text_value.and_then(Value::as_str) else {
        return Err((
            "缺少 new_text".to_string(),
            ToolErrorMeta::new(
                "TOOL_EDIT2_NEW_TEXT_REQUIRED",
                Some("请显式提供替换后的文本 new_text；删除文本时传空字符串。".to_string()),
                false,
                None,
            ),
        ));
    };
    let new_text = new_text.to_string();
    let expected_count = args
        .get("expected_count")
        .or_else(|| args.get("expectedCount"))
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let replace_all = args
        .get("replace_all")
        .or_else(|| args.get("replaceAll"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Some(EditInstruction {
        action: EditAction::Replace,
        old_text: Some(old_text),
        new_text,
        start_marker: None,
        end_marker: None,
        anchor: None,
        replace_all: replace_all || expected_count.is_some_and(|count| count > 1),
        expected_count,
        unique: expected_count.is_none() && !replace_all,
    }))
}

fn parse_edit_instructions(
    value: &Value,
) -> std::result::Result<Vec<EditInstruction>, (String, ToolErrorMeta)> {
    let items = match value {
        Value::Array(items) => items.as_slice(),
        Value::Object(_) => std::slice::from_ref(value),
        _ => {
            return Err((
                "edits 必须是对象或数组".to_string(),
                ToolErrorMeta::new(
                    "TOOL_EDIT2_INVALID_EDITS",
                    Some("请把 edits 写成对象，或由多个对象组成的数组。".to_string()),
                    false,
                    None,
                ),
            ));
        }
    };
    if items.is_empty() {
        return Err((
            "edits 不能为空".to_string(),
            ToolErrorMeta::new(
                "TOOL_EDIT2_EMPTY_EDITS",
                Some("至少提供一个编辑动作。".to_string()),
                false,
                None,
            ),
        ));
    }

    let mut output = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let obj = item.as_object().ok_or_else(|| {
            (
                format!("第 {} 个 edit 不是对象", index + 1),
                ToolErrorMeta::new(
                    "TOOL_EDIT2_INVALID_EDIT_ITEM",
                    Some("每个 edit 都必须是对象。".to_string()),
                    false,
                    None,
                ),
            )
        })?;
        let raw_action = obj
            .get("action")
            .or_else(|| obj.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let action = match raw_action.as_str() {
            "replace" => EditAction::Replace,
            "replace_between" | "replacebetween" => EditAction::ReplaceBetween,
            "insert_before" | "insertbefore" => EditAction::InsertBefore,
            "insert_after" | "insertafter" => EditAction::InsertAfter,
            "append" => EditAction::Append,
            "prepend" => EditAction::Prepend,
            _ => {
                return Err((
                    format!("第 {} 个 edit 的 action 无效：{}", index + 1, raw_action),
                    ToolErrorMeta::new(
                        "TOOL_EDIT2_INVALID_ACTION",
                        Some("只支持 replace / replace_between / insert_before / insert_after / append / prepend。".to_string()),
                        false,
                        None,
                    ),
                ));
            }
        };

        let new_text = obj
            .get("new_text")
            .or_else(|| obj.get("newText"))
            .or_else(|| obj.get("content"))
            .or_else(|| obj.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let old_text = obj
            .get("old_text")
            .or_else(|| obj.get("oldText"))
            .or_else(|| obj.get("find"))
            .or_else(|| obj.get("target"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let start_marker = obj
            .get("start_marker")
            .or_else(|| obj.get("startMarker"))
            .or_else(|| obj.get("before"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let end_marker = obj
            .get("end_marker")
            .or_else(|| obj.get("endMarker"))
            .or_else(|| obj.get("after"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let anchor = obj
            .get("anchor")
            .or_else(|| obj.get("marker"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let replace_all = obj
            .get("replace_all")
            .or_else(|| obj.get("replaceAll"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let expected_count = obj
            .get("expected_count")
            .or_else(|| obj.get("expectedCount"))
            .or_else(|| obj.get("count"))
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let unique = obj
            .get("unique")
            .and_then(Value::as_bool)
            .unwrap_or(matches!(
                action,
                EditAction::Replace
                    | EditAction::ReplaceBetween
                    | EditAction::InsertBefore
                    | EditAction::InsertAfter
            ));

        match action {
            EditAction::Replace => {
                if old_text.as_deref().unwrap_or("").is_empty() {
                    return Err((
                        format!("第 {} 个 replace 缺少 old_text", index + 1),
                        ToolErrorMeta::new(
                            "TOOL_EDIT2_OLD_TEXT_REQUIRED",
                            Some("replace 必须提供 old_text 和 new_text。".to_string()),
                            false,
                            None,
                        ),
                    ));
                }
            }
            EditAction::ReplaceBetween => {
                if start_marker.as_deref().unwrap_or("").is_empty()
                    || end_marker.as_deref().unwrap_or("").is_empty()
                {
                    return Err((
                        format!(
                            "第 {} 个 replace_between 缺少 start_marker 或 end_marker",
                            index + 1
                        ),
                        ToolErrorMeta::new(
                            "TOOL_EDIT2_MARKERS_REQUIRED",
                            Some(
                                "replace_between 必须同时提供 start_marker 和 end_marker。"
                                    .to_string(),
                            ),
                            false,
                            None,
                        ),
                    ));
                }
            }
            EditAction::InsertBefore | EditAction::InsertAfter => {
                if anchor.as_deref().unwrap_or("").is_empty() {
                    return Err((
                        format!("第 {} 个插入动作缺少 anchor", index + 1),
                        ToolErrorMeta::new(
                            "TOOL_EDIT2_ANCHOR_REQUIRED",
                            Some("insert_before 和 insert_after 必须提供 anchor。".to_string()),
                            false,
                            None,
                        ),
                    ));
                }
            }
            EditAction::Append | EditAction::Prepend => {}
        }

        output.push(EditInstruction {
            action,
            old_text,
            new_text,
            start_marker,
            end_marker,
            anchor,
            replace_all,
            expected_count,
            unique,
        });
    }
    Ok(output)
}

fn execute_edit_file2_plan(
    workspace: &crate::workspace::WorkspaceManager,
    user_id: &str,
    path_for_write: &str,
    allow_roots: &[PathBuf],
    instructions: &[EditInstruction],
    dry_run: bool,
    ensure_newline: bool,
) -> Result<EditFile2Outcome> {
    let target = resolve_tool_path(workspace, user_id, path_for_write, allow_roots)?;
    if target.exists() && target.is_dir() {
        return Err(anyhow!("target path is a directory"));
    }
    let existed = target.exists();
    let previous_text = if existed {
        std::fs::read_to_string(&target)?
    } else {
        String::new()
    };
    let previous_bytes = if existed {
        target.metadata().map(|meta| meta.len()).unwrap_or(0)
    } else {
        0
    };
    let mut current = previous_text.clone();
    let mut change_count = 0usize;
    let mut instruction_results = Vec::with_capacity(instructions.len());

    for instruction in instructions {
        let outcome = apply_instruction(&mut current, instruction)?;
        if outcome.changed {
            change_count += 1;
        }
        instruction_results.push(json!({
            "action": outcome.action,
            "changed": outcome.changed,
            "matches": outcome.matches,
            "bytes": outcome.inserted_bytes,
        }));
    }
    if change_count == 0 {
        return Err(anyhow!("没有产生任何实际修改"));
    }
    if ensure_newline && !current.ends_with('\n') {
        current.push('\n');
    }
    if current == previous_text {
        return Err(anyhow!("编辑后的文本与原文件完全相同"));
    }

    if !dry_run {
        let workspace_root = workspace.workspace_root(user_id);
        let default_workspace_target = workspace.resolve_path(user_id, path_for_write)?;
        if is_within_root(&workspace_root, &target)
            && normalize_path_for_compare(&normalize_target_path(&target))
                == normalize_path_for_compare(&normalize_target_path(&default_workspace_target))
        {
            workspace.write_file(user_id, path_for_write, &current, true)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomic_write_text(&target, &current)?;
        }
    }

    Ok(EditFile2Outcome {
        target,
        existed,
        previous_bytes,
        new_bytes: current.len(),
        change_count,
        instruction_results,
    })
}

fn apply_instruction(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    match instruction.action {
        EditAction::Replace => apply_replace(text, instruction),
        EditAction::ReplaceBetween => apply_replace_between(text, instruction),
        EditAction::InsertBefore => apply_insert_before(text, instruction),
        EditAction::InsertAfter => apply_insert_after(text, instruction),
        EditAction::Append => apply_append(text, instruction),
        EditAction::Prepend => apply_prepend(text, instruction),
    }
}

fn apply_replace(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    let old_text = instruction.old_text.as_deref().unwrap_or("");
    let matches = text.matches(old_text).count();
    validate_match_count("replace", matches, instruction)?;
    let replaced = if instruction.replace_all {
        text.replace(old_text, &instruction.new_text)
    } else {
        text.replacen(old_text, &instruction.new_text, 1)
    };
    let changed = replaced != *text;
    *text = replaced;
    Ok(EditInstructionOutcome {
        action: "replace",
        changed,
        matches,
        inserted_bytes: instruction.new_text.len(),
    })
}

fn apply_replace_between(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    let start_marker = instruction.start_marker.as_deref().unwrap_or("");
    let end_marker = instruction.end_marker.as_deref().unwrap_or("");
    let start_positions = find_all_positions(text, start_marker);
    validate_position_count(
        "replace_between.start_marker",
        start_positions.len(),
        instruction,
    )?;
    let start_pos = *start_positions
        .first()
        .ok_or_else(|| anyhow!("start_marker not found"))?;
    let after_start = start_pos + start_marker.len();
    let end_relative = text[after_start..]
        .find(end_marker)
        .ok_or_else(|| anyhow!("end_marker not found after start_marker"))?;
    let end_pos = after_start + end_relative;
    let mut output = String::with_capacity(text.len() + instruction.new_text.len());
    output.push_str(&text[..after_start]);
    output.push_str(&instruction.new_text);
    output.push_str(&text[end_pos..]);
    let changed = output != *text;
    *text = output;
    Ok(EditInstructionOutcome {
        action: "replace_between",
        changed,
        matches: 1,
        inserted_bytes: instruction.new_text.len(),
    })
}

fn apply_insert_before(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    let anchor = instruction.anchor.as_deref().unwrap_or("");
    let positions = find_all_positions(text, anchor);
    validate_position_count("insert_before.anchor", positions.len(), instruction)?;
    let pos = *positions
        .first()
        .ok_or_else(|| anyhow!("anchor not found"))?;
    let mut output = String::with_capacity(text.len() + instruction.new_text.len());
    output.push_str(&text[..pos]);
    output.push_str(&instruction.new_text);
    output.push_str(&text[pos..]);
    let changed = output != *text;
    *text = output;
    Ok(EditInstructionOutcome {
        action: "insert_before",
        changed,
        matches: 1,
        inserted_bytes: instruction.new_text.len(),
    })
}

fn apply_insert_after(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    let anchor = instruction.anchor.as_deref().unwrap_or("");
    let positions = find_all_positions(text, anchor);
    validate_position_count("insert_after.anchor", positions.len(), instruction)?;
    let pos = *positions
        .first()
        .ok_or_else(|| anyhow!("anchor not found"))?;
    let insert_at = pos + anchor.len();
    let mut output = String::with_capacity(text.len() + instruction.new_text.len());
    output.push_str(&text[..insert_at]);
    output.push_str(&instruction.new_text);
    output.push_str(&text[insert_at..]);
    let changed = output != *text;
    *text = output;
    Ok(EditInstructionOutcome {
        action: "insert_after",
        changed,
        matches: 1,
        inserted_bytes: instruction.new_text.len(),
    })
}

fn apply_append(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    let changed = !instruction.new_text.is_empty();
    text.push_str(&instruction.new_text);
    Ok(EditInstructionOutcome {
        action: "append",
        changed,
        matches: 1,
        inserted_bytes: instruction.new_text.len(),
    })
}

fn apply_prepend(
    text: &mut String,
    instruction: &EditInstruction,
) -> Result<EditInstructionOutcome> {
    let mut output = String::with_capacity(text.len() + instruction.new_text.len());
    output.push_str(&instruction.new_text);
    output.push_str(text);
    let changed = output != *text;
    *text = output;
    Ok(EditInstructionOutcome {
        action: "prepend",
        changed,
        matches: 1,
        inserted_bytes: instruction.new_text.len(),
    })
}

fn validate_match_count(kind: &str, matches: usize, instruction: &EditInstruction) -> Result<()> {
    if matches == 0 {
        return Err(anyhow!("{kind} target not found"));
    }
    if instruction.unique && matches != 1 {
        return Err(anyhow!("{kind} expected exactly 1 match, got {matches}"));
    }
    if let Some(expected_count) = instruction.expected_count {
        if matches != expected_count {
            return Err(anyhow!(
                "{kind} expected {expected_count} matches, got {matches}"
            ));
        }
    }
    Ok(())
}

fn validate_position_count(
    kind: &str,
    matches: usize,
    instruction: &EditInstruction,
) -> Result<()> {
    validate_match_count(kind, matches, instruction)
}

fn find_all_positions(text: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    text.match_indices(needle).map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceManager;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn parse_edit_instructions_accepts_single_object() {
        let items = parse_edit_instructions(&json!({
            "action": "replace",
            "old_text": "a",
            "new_text": "b"
        }))
        .expect("instructions");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, EditAction::Replace);
    }

    #[test]
    fn parse_flat_replace_prefers_single_exact_replacement() {
        let plan = parse_edit_file2_plan(&json!({
            "path": "demo.txt",
            "old_text": "old",
            "new_text": "new"
        }))
        .expect("plan");
        assert_eq!(plan.instructions.len(), 1);
        assert_eq!(plan.instructions[0].action, EditAction::Replace);
        assert_eq!(plan.instructions[0].old_text.as_deref(), Some("old"));
        assert_eq!(plan.instructions[0].new_text, "new");
        assert!(plan.instructions[0].unique);
    }

    #[test]
    fn parse_flat_replace_requires_explicit_new_text() {
        let err = parse_edit_file2_plan(&json!({
            "path": "demo.txt",
            "old_text": "old"
        }))
        .expect_err("missing new_text");
        assert_eq!(err.1.code, "TOOL_EDIT2_NEW_TEXT_REQUIRED");
    }

    #[test]
    fn parse_flat_replace_expected_count_replaces_all_expected_matches() {
        let plan = parse_edit_file2_plan(&json!({
            "path": "demo.txt",
            "old_text": "old",
            "new_text": "new",
            "expected_count": 2
        }))
        .expect("plan");
        assert!(plan.instructions[0].replace_all);
        assert_eq!(plan.instructions[0].expected_count, Some(2));
        assert!(!plan.instructions[0].unique);
    }

    #[test]
    fn apply_replace_rejects_non_unique_match_by_default() {
        let mut text = "a\na\n".to_string();
        let err = apply_replace(
            &mut text,
            &EditInstruction {
                action: EditAction::Replace,
                old_text: Some("a".to_string()),
                new_text: "b".to_string(),
                start_marker: None,
                end_marker: None,
                anchor: None,
                replace_all: false,
                expected_count: None,
                unique: true,
            },
        )
        .expect_err("should reject");
        assert!(err.to_string().contains("expected exactly 1 match"));
    }

    #[test]
    fn apply_replace_between_updates_middle_section() {
        let mut text = "a\nSTART\nold\nEND\nz\n".to_string();
        let result = apply_replace_between(
            &mut text,
            &EditInstruction {
                action: EditAction::ReplaceBetween,
                old_text: None,
                new_text: "new\n".to_string(),
                start_marker: Some("START\n".to_string()),
                end_marker: Some("END\n".to_string()),
                anchor: None,
                replace_all: false,
                expected_count: None,
                unique: true,
            },
        )
        .expect("replace_between");
        assert!(result.changed);
        assert_eq!(text, "a\nSTART\nnew\nEND\nz\n");
    }

    #[test]
    fn execute_edit_file2_plan_writes_changes() {
        let dir = tempdir().expect("tempdir");
        let workspace_root = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace_root).expect("workspace root");
        let storage = crate::storage::SqliteStorage::new(
            dir.path()
                .join("edit-file2.db")
                .to_string_lossy()
                .to_string(),
        );
        let workspace_root_text = workspace_root.to_string_lossy().to_string();
        let workspace =
            WorkspaceManager::new(&workspace_root_text, Arc::new(storage), 0, &HashMap::new());
        let user_id = "tester";
        let user_root = workspace.ensure_user_root(user_id).expect("user root");
        let file_path = user_root.join("demo.txt");
        std::fs::write(&file_path, "hello\nworld\n").expect("seed file");

        let outcome = execute_edit_file2_plan(
            &workspace,
            user_id,
            "demo.txt",
            std::slice::from_ref(&user_root),
            &[EditInstruction {
                action: EditAction::Replace,
                old_text: Some("world".to_string()),
                new_text: "wunder".to_string(),
                start_marker: None,
                end_marker: None,
                anchor: None,
                replace_all: false,
                expected_count: Some(1),
                unique: true,
            }],
            false,
            false,
        )
        .expect("outcome");

        assert!(outcome.existed);
        assert_eq!(
            std::fs::read_to_string(&file_path).expect("read back"),
            "hello\nwunder\n"
        );
    }
}
