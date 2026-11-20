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
use super::line_diff::{
    changed_lines, diff_literal_replaces, diff_replaced, insert_diff_fields, render_write_diff,
    DiffEcho, LineDiff,
};
use super::{
    build_model_tool_success,
    command_options::parse_dry_run,
    execute_in_sandbox, resolve_tool_path,
    tool_error::{build_failed_tool_result, ToolErrorMeta},
    touch_lsp_file, ToolContext,
};
use super::{edit_batch, edit_match, edit_recovery, file_history, write_receipt};
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
pub(super) enum EditFailure {
    NotFound,
    NoMatch {
        nearest: Vec<edit_recovery::Nearest>,
    },
    NotUnique {
        count: usize,
        lines: Vec<usize>,
    },
}

impl std::fmt::Display for EditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditFailure::NotFound => write!(f, "目标文件不存在"),
            EditFailure::NoMatch { .. } => write!(f, "old_string 未匹配"),
            EditFailure::NotUnique { count, .. } => write!(f, "old_string 匹配到 {count} 处"),
        }
    }
}

impl std::error::Error for EditFailure {}

struct EditOutcome {
    target: PathBuf,
    occurrences: usize,
    bytes_before: usize,
    bytes_after: usize,
    /// 字面替换或行区间替换的真实 diff；`unavailable` 有值时只带计数不带块。
    diff: LineDiff,
    unavailable: Option<&'static str>,
    changed: bool,
    /// 改前/改后全文，只为 unified diff 与回执服务。
    before: String,
    after: String,
    snapshot: Option<file_history::Snapshot>,
    receipt: write_receipt::Receipt,
    range: Option<(usize, usize)>,
    /// `anchor` 把行区间整体挪了几行（仅行区间模式、且锚点纠正过位置时有值）。
    range_shift: Option<i64>,
    /// 定位档位：`exact` 之外说明这次替换是靠空白容错命中的。
    tier: edit_match::MatchTier,
    /// 批量替换（`edits`）的逐条报告；单条替换为 `None`。
    batch: Option<edit_batch::BatchReport>,
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

    // 批量替换：只给 file_path + edits（+ 可选 on_miss），与其它编辑形态互斥。
    if edit_batch::batch_requested(args) {
        return edit_batch::validate_batch_args(args);
    }

    // 行区间替换：start_line/end_line + new_string，与 old_string 互斥。
    let range_mode = args.get("start_line").is_some() || args.get("end_line").is_some();
    if range_mode {
        let start = args.get("start_line").and_then(Value::as_u64);
        let end = args.get("end_line").and_then(Value::as_u64);
        if start.is_none() || end.is_none() {
            return Some(build_failed_tool_result(
                "行区间替换需要同时给出 start_line 与 end_line",
                json!({"file_path": file_path, "range_incomplete": true}),
                ToolErrorMeta::new(
                    "TOOL_EDIT_RANGE_INCOMPLETE",
                    Some("start_line/end_line 都是 1-based 整数，区间含两端。".to_string()),
                    false,
                    None,
                ),
                false,
            ));
        }
        if start == Some(0) {
            return Some(build_failed_tool_result(
                "start_line 从 1 开始",
                json!({"file_path": file_path, "range_invalid": true}),
                ToolErrorMeta::new(
                    "TOOL_EDIT_RANGE_INVALID",
                    Some(
                        "行号从 1 开始；要插到文件开头请用 command=insert 且 insert_line=0。"
                            .to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
        if args.get("old_string").is_some() {
            return Some(build_failed_tool_result(
                "start_line/end_line 与 old_string 不能同时使用",
                json!({"file_path": file_path, "range_conflict": true}),
                ToolErrorMeta::new(
                    "TOOL_EDIT_RANGE_CONFLICT",
                    Some("行区间替换只给 start_line/end_line + new_string；字面替换只给 old_string + new_string。"
                        .to_string()),
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
                    Some(
                        "行区间替换必须给出字符串 new_string；空字符串表示删除这些行。".to_string(),
                    ),
                    false,
                    None,
                ),
                false,
            ));
        }
        return None;
    }

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
/// 先归一化别名与校验类型，再将互斥的补丁、子命令、字面替换分派。
/// `command=str_replace` 与省略 command 共用同一替换实现。
pub(crate) async fn text_edit(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let args = match super::edit_args::normalize_edit_args(args) {
        Ok(args) => args,
        Err(failure) => return Ok(failure),
    };
    if has_patch_input(&args) {
        return super::apply_patch_tool::apply_patch(context, &args).await;
    }
    if super::str_replace_editor_tool::command_requested(&args)
        && args.get("command").and_then(Value::as_str) != Some("str_replace")
    {
        return super::str_replace_editor_tool::str_replace_editor(context, &args).await;
    }
    edit_file(context, &args).await
}

async fn edit_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if let Some(failure) = validate_edit_args(args) {
        return Ok(failure);
    }
    if let Some(result) = execute_in_sandbox(context, TOOL_EDIT, args).await {
        if sandbox_edit_mutated(&result, args) {
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
    let verify = args.get("verify").and_then(Value::as_bool).unwrap_or(true);
    let range = parse_range(args);
    let anchor = parse_anchor(args);
    let batch = if edit_batch::batch_requested(&args) {
        edit_batch::parse_batch(&args)
    } else {
        None
    };
    let batch_continue = edit_batch::continue_on_miss(&args);

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
        let (prepared, batch_report, resolved_range) = match (&batch, range) {
            (Some(steps), _) => {
                let (prepared, report) =
                    edit_batch::prepare(&content, steps, batch_continue).map_err(anyhow::Error::new)?;
                (prepared, Some(report), None)
            }
            (None, Some((start_line, end_line))) => {
                // 行号 + 可选 anchor：文件漂移时按锚点实际行号整体平移区间。
                let (prepared, resolved) = prepare_range_edit(
                    &content,
                    start_line,
                    end_line,
                    &new_for_edit,
                    anchor.as_deref(),
                )?;
                (prepared, None, Some(resolved))
            }
            (None, None) => (
                prepare_literal_edit(&content, &old_for_edit, &new_for_edit, replace_all)?,
                None,
                None,
            ),
        };
        let workspace_root = workspace.workspace_root(&user_id);
        // 覆盖前的快照：写坏了能 diff 回滚；失败只是没有历史，不影响写入。
        let snapshot = if !dry_run && prepared.changed {
            file_history::snapshot(&target, &workspace_root, &content)
        } else {
            None
        };
        if !dry_run && prepared.changed {
            let default_target = workspace.resolve_path(&user_id, &path_for_edit)?;
            if is_within_root(&workspace_root, &target)
                && normalize_path_for_compare(&normalize_target_path(&target))
                    == normalize_path_for_compare(&normalize_target_path(&default_target))
            {
                workspace.write_file(&user_id, &path_for_edit, &prepared.updated, true)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs_cap.create_dir_all(parent)?;
                }
                fs_cap.write_text(&target, &prepared.updated)?;
            }
        }
        let receipt = write_receipt::build(&path_for_edit, &prepared.updated, verify);
        Ok::<EditOutcome, anyhow::Error>(EditOutcome {
            target,
            occurrences: prepared.occurrences,
            bytes_before: content.len(),
            bytes_after: prepared.updated.len(),
            diff: prepared.diff,
            unavailable: prepared.unavailable,
            changed: prepared.changed,
            before: content,
            after: prepared.updated,
            snapshot,
            receipt,
            range: resolved_range.map(|resolved| (resolved.start, resolved.end)).or(range),
            range_shift: resolved_range
                .map(|resolved| resolved.shift)
                .filter(|shift| *shift != 0),
            tier: prepared.tier,
            batch: batch_report,
        })
    })
    .await;

    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(err) => {
            if let Some(failure) = err.downcast_ref::<EditFailure>() {
                return Ok(edit_failure_result(&file_path, dry_run, failure));
            }
            if let Some(failure) = err.downcast_ref::<edit_batch::BatchEditFailure>() {
                return Ok(edit_batch::failure_result(&file_path, dry_run, failure));
            }
            if let Some(failure) = err.downcast_ref::<RangeFailure>() {
                let mut data = Map::<String, Value>::new();
                data.insert("path".to_string(), json!(file_path));
                data.insert("dry_run".to_string(), json!(dry_run));
                data.insert("file_lines".to_string(), json!(failure.total_lines()));
                // 锚点失败与行号越界是两类问题：给不同的 code 和不同的改法，
                // 否则模型会把「anchor 找不到」当成「行号填错了」继续试。
                let (code, hint) = match failure {
                    RangeFailure::AnchorMissing { nearest } => {
                        data.insert("nearest".to_string(), edit_recovery::to_json(nearest));
                        (
                            "TOOL_EDIT_ANCHOR_MISSING",
                            "anchor 要填区间第一行的当前原文；按返回的 nearest 校正，或去掉 anchor 直接用行号。锚点与内容都对不上时请先读该区域。",
                        )
                    }
                    RangeFailure::AnchorAmbiguous { count, lines } => {
                        data.insert("matches".to_string(), json!(count));
                        data.insert("lines".to_string(), json!(lines));
                        (
                            "TOOL_EDIT_ANCHOR_NOT_UNIQUE",
                            "anchor 要能唯一定位：带上整行上下文（例如连同行首缩进与函数名），或改用 old_string 做字面替换。",
                        )
                    }
                    _ => (
                        "TOOL_EDIT_RANGE_INVALID",
                        "按返回的 file_lines 修正 start_line/end_line；行区间为 1-based 且含两端。",
                    ),
                };
                // 失败结果在观察层会被压成「只剩 error 文本」，定位要点必须进正文。
                let message = match failure {
                    RangeFailure::AnchorMissing { nearest } => {
                        let located = nearest
                            .first()
                            .map(|nearest| {
                                format!(
                                    "最近片段在第 {} 行（相似度 {}%）",
                                    nearest.line, nearest.similarity
                                )
                            })
                            .unwrap_or_else(|| "全文没有相近行".to_string());
                        format!("文本编辑失败：anchor 在当前文件里找不到，{located}")
                    }
                    RangeFailure::AnchorAmbiguous { count, lines } => format!(
                        "文本编辑失败：anchor 匹配到 {count} 处（行号 {lines:?}），未修改文件"
                    ),
                    other => format!("文本编辑失败：{other}"),
                };
                return Ok(build_failed_tool_result(
                    message,
                    Value::Object(data),
                    ToolErrorMeta::new(code, Some(hint.to_string()), false, None),
                    false,
                ));
            }
            return Ok(build_failed_tool_result(
                format!("文本编辑失败：{}", err),
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

    if !dry_run && outcome.changed {
        context.workspace.mark_tree_dirty(context.workspace_id);
    }
    let lsp_info = if dry_run || !outcome.changed {
        Value::Null
    } else {
        touch_lsp_file(context, &outcome.target, true).await
    };

    // 编辑落盘的内容就是这次调用发出去的（old_string/new_string、行区间替换），
    // 回灌 diff 没有信息增量：默认只回回执 + `changed_lines`。`dry_run` 例外——
    // 预览问的就是「会变成什么样」。
    let echo = if dry_run {
        DiffEcho::Full
    } else {
        DiffEcho::Authored
    };
    let (unified, unified_truncated, omitted_reason) = render_write_diff(
        echo,
        &file_path,
        outcome.receipt.lines,
        &outcome.before,
        &outcome.after,
        &outcome.diff,
    );
    let mut data = match build_edit_data(
        &file_path,
        outcome.occurrences,
        replace_all,
        outcome.bytes_before,
        outcome.bytes_after,
        dry_run,
        lsp_info,
        &outcome.diff,
        outcome.unavailable,
        &outcome.receipt,
        outcome.snapshot.as_ref(),
        outcome.range,
        &unified,
        unified_truncated,
        omitted_reason,
    ) {
        Value::Object(map) => map,
        other => return Ok(other),
    };
    if !outcome.changed {
        data["changed_files"] = json!(0);
        data["updated"] = json!(0);
    }
    // 定位档位常驻一个键：`exact` 之外说明这次靠空白容错命中，模型据此决定
    // 要不要回读确认（取值是固定几个枚举字符串，键集不随情况变化）。
    data.insert("match".to_string(), json!(outcome.tier.as_str()));
    if !outcome.tier.is_exact() {
        data.insert("match_note".to_string(), json!(outcome.tier.note()));
    }
    if let Some(shift) = outcome.range_shift {
        // 行号被锚点纠正过：明确告诉模型「你的行号变了多少」，避免它按旧行号再改一次。
        data.insert("range_shift".to_string(), json!(shift));
    }
    if let Some(report) = &outcome.batch {
        data.insert("edits".to_string(), report.steps.clone());
        data.insert("edits_applied".to_string(), json!(report.applied));
        data.insert("edits_missed".to_string(), json!(report.missed));
        if report.is_partial() {
            data.insert("partial".to_string(), json!(true));
        }
    }

    let state = if dry_run {
        "dry_run"
    } else if !outcome.changed {
        "unchanged"
    } else {
        "completed"
    };
    let summary = if dry_run {
        format!("Validated edit against {file_path} without writing content.")
    } else if !outcome.changed {
        format!("No content change in {file_path}.")
    } else if let Some(report) = &outcome.batch {
        let total = report.steps.as_array().map(Vec::len).unwrap_or(0);
        let missed = if report.is_partial() {
            format!(", {} missed — see edits", report.missed)
        } else {
            String::new()
        };
        format!(
            "Edited {file_path} ({} of {total} edits applied{}); {} lines, sha256 {}, syntax {}.",
            report.applied,
            missed,
            outcome.receipt.lines,
            outcome.receipt.sha256,
            outcome.receipt.syntax
        )
    } else if outcome.tier.is_exact() {
        format!(
            "Edited {file_path} ({} replacement(s)); {} lines, sha256 {}, syntax {}.",
            outcome.occurrences,
            outcome.receipt.lines,
            outcome.receipt.sha256,
            outcome.receipt.syntax
        )
    } else {
        format!(
            "Edited {file_path} ({} replacement(s), matched by {}); {} lines, sha256 {}, syntax {}.",
            outcome.occurrences,
            outcome.tier.as_str(),
            outcome.receipt.lines,
            outcome.receipt.sha256,
            outcome.receipt.syntax
        )
    };
    Ok(build_model_tool_success(
        "edit",
        state,
        summary,
        Value::Object(data),
    ))
}

/// 一次编辑的「改后状态」：字面替换与行区间替换共用。
#[derive(Debug)]
pub(super) struct PreparedEdit {
    pub(super) updated: String,
    pub(super) occurrences: usize,
    pub(super) diff: LineDiff,
    pub(super) unavailable: Option<&'static str>,
    pub(super) changed: bool,
    /// 字面替换用的匹配档位；行区间替换恒为 `Exact`。
    pub(super) tier: edit_match::MatchTier,
}

/// 失败时最多回几个最相似片段。
const NEAREST_MATCH_LIMIT: usize = 3;

/// 一次字面替换的纯计算：定位（含容错档位）→ 唯一性校验 → 生成新内容。
///
/// 拆成独立函数是为了让批量编辑在同一份内容上连续套用**同一套**语义：
/// 定位放宽规则、唯一性判定、替换拼接只有一处实现。
pub(super) fn replace_literal(
    content: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<(String, edit_match::Found), EditFailure> {
    let old_text = match_text_newlines(content, old);
    let new_text = match_text_newlines(content, new);
    let Some(found) = edit_match::find(content, old_text.as_ref()) else {
        return Err(EditFailure::NoMatch {
            nearest: edit_recovery::nearest_matches(
                content,
                old_text.as_ref(),
                NEAREST_MATCH_LIMIT,
            ),
        });
    };
    if found.occurrences() > 1 && !replace_all {
        return Err(EditFailure::NotUnique {
            count: found.occurrences(),
            lines: match_line_numbers(content, &found.starts()),
        });
    }
    let updated = edit_match::replace(content, &found, new_text.as_ref(), replace_all);
    Ok((updated, found))
}

fn prepare_literal_edit(
    content: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<PreparedEdit, anyhow::Error> {
    let (updated, found) = replace_literal(content, old, new, replace_all).map_err(anyhow::Error::new)?;
    let occurrences = found.occurrences();
    let changed = content != updated;
    let (diff, unavailable) = if !changed {
        (LineDiff::default(), None)
    } else if found.tier.is_exact() {
        // 逐字命中时按命中起点采样 diff 块，能省一次全文比对。
        let offsets = found.starts();
        let sampled = &offsets[..offsets.len().min(MAX_DIFF_OFFSETS)];
        let needle_len = found.spans[0].1 - found.spans[0].0;
        let new_text = match_text_newlines(content, new);
        build_replace_diff(
            content,
            &updated,
            sampled,
            occurrences,
            needle_len,
            new_text.as_ref(),
        )
    } else {
        // 容错命中时命中区间长度与 old_string 不等长，只能整块重算。
        (diff_replaced(content, &updated), None)
    };
    Ok(PreparedEdit {
        updated,
        occurrences,
        diff,
        unavailable,
        changed,
        tier: found.tier,
    })
}

fn prepare_range_edit(
    content: &str,
    start_line: usize,
    end_line: usize,
    new_text: &str,
    anchor: Option<&str>,
) -> Result<(PreparedEdit, ResolvedRange), anyhow::Error> {
    let resolved = resolve_range(content, start_line, end_line, anchor)?;
    let updated = apply_range_replace(content, resolved.start, resolved.end, new_text)
        .map_err(anyhow::Error::new)?;
    let changed = content != updated;
    let diff = if changed {
        diff_replaced(content, &updated)
    } else {
        LineDiff::default()
    };
    Ok((
        PreparedEdit {
            updated,
            occurrences: 1,
            diff,
            unavailable: None,
            changed,
            tier: edit_match::MatchTier::Exact,
        },
        resolved,
    ))
}

/// 行区间编辑实际落在哪里：请求行号 + 锚点校正后的偏移。
#[derive(Debug, Clone, Copy)]
struct ResolvedRange {
    start: usize,
    end: usize,
    /// 锚点把区间整体挪了几行（正数向下，负数向上，0 表示没挪）。
    shift: i64,
}

/// 把「行号」加上「内容锚点」再核一次位置。
///
/// 源问题：`start_line/end_line` 是读文件那一刻的快照，文件一被改动就整体漂移，
/// 模型只能重新 grep 行号。这里允许额外给 `anchor`（区间第一行的当前原文）：
/// 命中就按实际行号整体平移区间，并把偏移量回报出来；锚点找不到或命中多处则拒绝，
/// 绝不猜位置。
fn resolve_range(
    content: &str,
    start_line: usize,
    end_line: usize,
    anchor: Option<&str>,
) -> Result<ResolvedRange, RangeFailure> {
    let Some(anchor) = anchor.filter(|text| !text.trim().is_empty()) else {
        return Ok(ResolvedRange {
            start: start_line,
            end: end_line,
            shift: 0,
        });
    };
    let Some(found) = edit_match::find(content, anchor) else {
        return Err(RangeFailure::AnchorMissing {
            nearest: edit_recovery::nearest_matches(content, anchor, 1),
        });
    };
    if found.occurrences() > 1 {
        return Err(RangeFailure::AnchorAmbiguous {
            count: found.occurrences(),
            lines: match_line_numbers(content, &found.starts()),
        });
    }
    let anchor_line = line_number_at(content, found.spans[0].0);
    let shift = anchor_line as i64 - start_line as i64;
    // 区间起点就是锚点所在行（锚点 = 区间第一行的当前原文），终点按同一偏移平移。
    let shifted_end = end_line as i64 + shift;
    if shifted_end < anchor_line as i64 {
        return Err(RangeFailure::EndBeforeStart);
    }
    Ok(ResolvedRange {
        start: anchor_line,
        end: shifted_end as usize,
        shift,
    })
}

/// 字节偏移落在第几行（1-based）。
fn line_number_at(content: &str, offset: usize) -> usize {
    1 + content.as_bytes()[..offset.min(content.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
}

fn parse_anchor(args: &Value) -> Option<String> {
    args.get("anchor")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_range(args: &Value) -> Option<(usize, usize)> {
    let start = args.get("start_line").and_then(Value::as_u64)?;
    let end = args.get("end_line").and_then(Value::as_u64)?;
    Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
}

/// 行区间替换的失败原因：带上真实行数，模型一次就能改对。
#[derive(Debug, Clone)]
enum RangeFailure {
    StartBelowOne,
    EndBeforeStart,
    BeyondFile {
        requested_end: usize,
        lines: usize,
    },
    /// 给了 `anchor` 但当前文件里找不到它：这段内容可能被改名或删掉了。
    AnchorMissing {
        nearest: Vec<edit_recovery::Nearest>,
    },
    /// `anchor` 命中多处：定位有歧义，宁可报错也不猜。
    AnchorAmbiguous {
        count: usize,
        lines: Vec<usize>,
    },
}

impl RangeFailure {
    fn total_lines(&self) -> usize {
        match self {
            RangeFailure::BeyondFile { lines, .. } => *lines,
            _ => 0,
        }
    }
}

impl std::fmt::Display for RangeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RangeFailure::StartBelowOne => write!(f, "start_line 从 1 开始，不能为 0"),
            RangeFailure::EndBeforeStart => write!(f, "end_line 必须不小于 start_line"),
            RangeFailure::BeyondFile {
                requested_end,
                lines,
            } => write!(
                f,
                "end_line={} 超出文件行数（文件共 {} 行）",
                requested_end, lines
            ),
            RangeFailure::AnchorMissing { .. } => write!(f, "anchor 在当前文件里找不到"),
            RangeFailure::AnchorAmbiguous { count, .. } => {
                write!(f, "anchor 匹配到 {count} 处，无法唯一定位")
            }
        }
    }
}

impl std::error::Error for RangeFailure {}

/// 按行区间替换：`start_line..=end_line`（1-based，含两端）整体换成 `new_text`。
///
/// 行尾语义按原文保持：被替换的最后一行原本有换行，就保证新文本也以换行结束；
/// 替换到文件末尾且原文末尾无换行，则去掉多余换行，避免悄悄改动 EOF。
fn apply_range_replace(
    content: &str,
    start_line: usize,
    end_line: usize,
    new_text: &str,
) -> Result<String, RangeFailure> {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let total = lines.len();
    if start_line == 0 {
        return Err(RangeFailure::StartBelowOne);
    }
    if end_line < start_line {
        return Err(RangeFailure::EndBeforeStart);
    }
    if start_line > total || end_line > total {
        return Err(RangeFailure::BeyondFile {
            requested_end: end_line,
            lines: total,
        });
    }
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut segment = new_text.replace("\r\n", "\n");
    if newline == "\r\n" {
        segment = segment.replace('\n', "\r\n");
    }
    let keeps_trailing = lines[end_line - 1].ends_with('\n');
    if keeps_trailing {
        if !segment.is_empty() && !segment.ends_with('\n') {
            segment.push_str(newline);
        }
    } else {
        while segment.ends_with('\n') || segment.ends_with('\r') {
            segment.pop();
        }
    }
    let mut out = String::with_capacity(content.len() + segment.len());
    out.push_str(&lines[..start_line - 1].concat());
    out.push_str(&segment);
    out.push_str(&lines[end_line..].concat());
    Ok(out)
}

/// 失败载荷：`nearest` 里带行区间、预览与逐行差异，模型据此可直接自愈。
fn edit_failure_result(path: &str, dry_run: bool, failure: &EditFailure) -> Value {
    let mut data = Map::<String, Value>::new();
    data.insert("path".to_string(), json!(path));
    data.insert("dry_run".to_string(), json!(dry_run));
    let (message, code, hint) = match failure {
        EditFailure::NotFound => (
            format!("文本编辑失败：文件不存在 {}", path),
            "TOOL_EDIT_NOT_FOUND",
            "确认路径正确；新建文件请改用写入文件。".to_string(),
        ),
        EditFailure::NoMatch { nearest } => {
            let mut message = format!("文本编辑失败：old_string 未匹配 {}", path);
            let mut hint = "已按行尾空白、块内缩进、行内空白、去掉全部空白逐级放宽仍不匹配，差异在内容本身（数值、标识符、引号、大小写）；按 nearest 的逐行差异校正，或改用 start_line/end_line + anchor（该行当前原文）做行区间替换，行号漂移会被自动纠正。".to_string();
            if let Some(summary) = edit_recovery::summary_line(nearest) {
                message.push_str("；");
                message.push_str(&summary);
            }
            if let Some(first) = nearest.first() {
                hint = format!(
                    "用 replace_range 改这一处：start_line={}, end_line={}（行区间 1-based 含两端）；确需字面替换时按 nearest.snippet 校正空白与引号。",
                    first.line, first.end_line
                );
            }
            data.insert("nearest".to_string(), edit_recovery::to_json(nearest));
            (message, "TOOL_EDIT_NO_MATCH", hint)
        }
        EditFailure::NotUnique { count, lines } => (
            format!("old_string 匹配到 {count} 处（行号 {lines:?}），未修改文件"),
            "TOOL_EDIT_NOT_UNIQUE",
            "补充上下文使匹配唯一，或设置 replace_all=true，或改用 start_line/end_line + anchor（首行当前原文）精确替换。"
                .to_string(),
        ),
    };
    build_failed_tool_result(
        message,
        Value::Object(data),
        ToolErrorMeta::new(code, Some(hint), false, None),
        false,
    )
}

pub(super) fn sandbox_edit_mutated(result: &Value, args: &Value) -> bool {
    let inner = super::tool_result_data(result);
    result.get("ok").and_then(Value::as_bool) == Some(true)
        && inner.get("ok").and_then(Value::as_bool) != Some(false)
        && !parse_dry_run(args)
        && super::tool_result_field(inner, "changed_files")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
}

/// Only line-ending normalization is allowed; indentation and other whitespace
/// remain literal. Keep replacement text in the file's existing newline style.
pub(super) fn match_text_newlines<'a>(content: &str, text: &'a str) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow;
    let crlf = content.contains("\r\n")
        && content
            .as_bytes()
            .windows(2)
            .all(|pair| pair[1] != b'\n' || pair[0] == b'\r')
        && !content.starts_with('\n');
    if crlf && text.contains('\n') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else if content.contains('\n') && !content.contains('\r') && text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

fn match_line_numbers(content: &str, offsets: &[usize]) -> Vec<usize> {
    let mut cursor = 0;
    let mut line = 1;
    offsets
        .iter()
        .take(8)
        .map(|&offset| {
            line += content.as_bytes()[cursor..offset]
                .iter()
                .filter(|b| **b == b'\n')
                .count();
            cursor = offset;
            line
        })
        .collect()
}

/// 组装编辑结果 `data`：既有键保持不变，回执、历史快照与 unified diff 一并给出。
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
    receipt: &write_receipt::Receipt,
    snapshot: Option<&file_history::Snapshot>,
    range: Option<(usize, usize)>,
    unified: &str,
    unified_truncated: bool,
    omitted_reason: Option<&'static str>,
) -> Value {
    let mut data = Map::<String, Value>::new();
    data.insert("path".to_string(), json!(path));
    data.insert("replacements".to_string(), json!(occurrences));
    data.insert("replace_all".to_string(), json!(replace_all));
    data.insert("bytes_before".to_string(), json!(bytes_before));
    data.insert("bytes_after".to_string(), json!(bytes_after));
    data.insert("dry_run".to_string(), json!(dry_run));
    data.insert("lsp".to_string(), lsp);
    insert_diff_fields(&mut data, "update", path, diff, unavailable);
    if let Some(reason) = unavailable {
        data.insert("diff_unavailable".to_string(), json!(reason));
    }
    if let Some((start_line, end_line)) = range {
        data.insert("range".to_string(), json!([start_line, end_line]));
    }
    if let Some(snapshot) = snapshot {
        data.insert("history".to_string(), json!(snapshot.display));
        data.insert("history_rev".to_string(), json!(snapshot.rev));
    }
    write_receipt::insert_fields(&mut data, receipt);
    data.insert("unified_diff".to_string(), json!(unified));
    data.insert(
        "unified_diff_truncated".to_string(),
        json!(unified_truncated),
    );
    if let Some(reason) = omitted_reason {
        data.insert("diff_omitted".to_string(), json!(reason));
        // 没有正文时，`changed_lines` 就是「改在哪」的答案。
        data.insert("changed_lines".to_string(), json!(changed_lines(diff)));
    }
    Value::Object(data)
}

#[cfg(test)]
#[cfg(feature = "sqlite-storage")]
#[path = "edit_tool_tests.rs"]
mod dispatch_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sandbox_dirty_flag_requires_a_successful_real_change() {
        let result = json!({"ok":true, "data":{"ok":true, "data":{"changed_files":1}}});
        assert!(sandbox_edit_mutated(&result, &json!({})));
        assert!(!sandbox_edit_mutated(&result, &json!({"dry_run":true})));
        for result in [
            json!({"ok":false, "data":{"ok":true, "data":{"changed_files":1}}}),
            json!({"ok":true, "data":{"ok":false, "data":{"changed_files":1}}}),
            json!({"ok":true, "data":{"ok":true, "data":{"changed_files":0}}}),
            json!({"ok":true, "data":{"ok":true, "data":{"content":"view"}}}),
        ] {
            assert!(!sandbox_edit_mutated(&result, &json!({})));
        }
    }

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
        let receipt = write_receipt::build("a.txt", after, false);
        let (unified, truncated, omitted) = render_write_diff(
            DiffEcho::Authored,
            "a.txt",
            receipt.lines,
            before,
            after,
            &diff,
        );
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
            &receipt,
            None,
            None,
            &unified,
            truncated,
            omitted,
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
        let receipt = write_receipt::build("a.txt", "", false);
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
            &receipt,
            None,
            None,
            "",
            false,
            None,
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
        let receipt = write_receipt::build("a.txt", "", false);
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
            &receipt,
            None,
            None,
            "",
            false,
            None,
        );
        assert_eq!(data["diff_unavailable"], json!("oversize"));
        // 超限时计数是「没算」而不是「0 行」，否则会被读成「改了个寂寞」。
        assert_eq!(data["added_lines"], Value::Null);
        assert_eq!(data["deleted_lines"], Value::Null);
        assert_eq!(data["hunks_applied"], Value::Null);
        assert_eq!(data["files"][0]["added_lines"], Value::Null);
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
                "changed_lines",
                "check",
                "deleted",
                "deleted_lines",
                "diff_lines_omitted",
                "diff_omitted",
                "dry_run",
                "files",
                "hunks_applied",
                "lines",
                "lsp",
                "moved",
                "path",
                "receipt",
                "replace_all",
                "replacements",
                "sha256",
                "syntax",
                "unified_diff",
                "unified_diff_truncated",
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
                "diff_omitted",
                "hunks",
                "path",
                "to_path"
            ]
        );
    }

    #[test]
    fn anchor_relocates_the_range_when_the_file_shifted() {
        let content = "a\nb\nc\nd\ne\n";
        // 请求 2..=3（b、c），但区间首行现在实际在第 4 行（d）→ 整体下移 2 行。
        let resolved = resolve_range(content, 2, 3, Some("d")).expect("resolved");
        assert_eq!(resolved.start, 4);
        assert_eq!(resolved.end, 5);
        assert_eq!(resolved.shift, 2);
    }

    #[test]
    fn anchor_missing_is_rejected_instead_of_guessed() {
        let failure = resolve_range("a\nb\n", 1, 1, Some("zzz")).expect_err("missing");
        assert!(matches!(failure, RangeFailure::AnchorMissing { .. }));
    }

    #[test]
    fn anchor_must_be_unique_to_relocate() {
        let failure = resolve_range("a\nb\na\n", 1, 1, Some("a")).expect_err("ambiguous");
        match failure {
            RangeFailure::AnchorAmbiguous { count, .. } => assert_eq!(count, 2),
            other => panic!("unexpected failure: {other:?}"),
        }
    }

    #[test]
    fn without_anchor_the_requested_range_is_kept() {
        let resolved = resolve_range("a\nb\n", 1, 2, None).expect("fixed");
        assert_eq!((resolved.start, resolved.end, resolved.shift), (1, 2, 0));
        // 空锚点等同没给。
        let blank = resolve_range("a\nb\n", 2, 2, Some("  ")).expect("fixed");
        assert_eq!((blank.start, blank.end, blank.shift), (2, 2, 0));
    }

    #[test]
    fn range_replace_swaps_inclusive_lines() {
        let updated = apply_range_replace("a\nb\nc\nd\n", 2, 3, "B\nC\n").expect("range");
        assert_eq!(updated, "a\nB\nC\nd\n");
    }

    #[test]
    fn range_replace_keeps_eof_newline_semantics() {
        assert_eq!(
            apply_range_replace("a\nb", 2, 2, "B").expect("range"),
            "a\nB"
        );
        assert_eq!(
            apply_range_replace("a\nb\n", 2, 2, "B").expect("range"),
            "a\nB\n"
        );
        assert_eq!(
            apply_range_replace("a\nb\n", 1, 2, "Z\n").expect("range"),
            "Z\n"
        );
    }

    #[test]
    fn range_replace_handles_crlf_and_deletions() {
        assert_eq!(
            apply_range_replace("a\r\nb\r\nc\r\n", 2, 2, "B\nX\n").expect("range"),
            "a\r\nB\r\nX\r\nc\r\n"
        );
        assert_eq!(
            apply_range_replace("a\nb\nc\n", 2, 2, "").expect("range"),
            "a\nc\n"
        );
    }

    #[test]
    fn range_replace_reports_the_real_line_count() {
        let failure = apply_range_replace("a\nb\n", 5, 6, "x").expect_err("out of range");
        assert!(failure.to_string().contains("文件共 2 行"), "{}", failure);
        assert!(apply_range_replace("a\n", 0, 1, "x").is_err());
        assert!(apply_range_replace("a\n", 2, 1, "x").is_err());
    }

    #[test]
    fn range_mode_is_validated_before_touching_disk() {
        assert!(validate_edit_args(&json!({
            "file_path": "a.py",
            "start_line": 2,
            "end_line": 4,
            "new_string": "x"
        }))
        .is_none());
        let conflict = validate_edit_args(&json!({
            "file_path": "a.py",
            "start_line": 2,
            "end_line": 4,
            "old_string": "y",
            "new_string": "x"
        }))
        .expect("conflict rejected");
        assert_eq!(
            conflict["error_meta"]["code"],
            json!("TOOL_EDIT_RANGE_CONFLICT")
        );
        assert!(validate_edit_args(&json!({
            "file_path": "a.py",
            "start_line": 2,
            "new_string": "x"
        }))
        .is_some());
    }

    #[test]
    fn no_match_failure_carries_nearest_fragments() {
        let failure = prepare_literal_edit(
            "def draw(t):\n    return t + 1\n",
            "def draw(t):\n    return t + 2",
            "x",
            false,
        )
        .expect_err("no match");
        let failure = failure.downcast_ref::<EditFailure>().expect("edit failure");
        let value = edit_failure_result("a.py", false, failure);
        let nearest = value["data"]["nearest"].as_array().expect("nearest array");
        assert!(!nearest.is_empty());
        assert_eq!(nearest[0]["line"], json!(1));
        assert!(nearest[0]["differences"].as_array().is_some());
        assert!(
            value["error"].as_str().unwrap_or("").contains("相似"),
            "{}",
            value["error"]
        );
    }
}
