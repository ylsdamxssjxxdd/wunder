//! `edits` 批量编辑：一次读、一次写、逐条报 MISS。
//!
//! 源问题：一次改 N 处只能发 N 次 `edit` 调用，任何一处 `old_string` 对不上，
//! 模型就得停下来重新定位，再发一轮；为了批量，人们干脆去写「python 补丁脚本 +
//! `in` 判断 + 打印 MISS」，但那既绕开了写入回执，也绕开了原子落盘与历史快照。
//!
//! 这里把那个脚本的正确部分搬进工具：在同一份内容上按顺序套用 N 条替换，
//! 每条复用 [`super::edit_tool::replace_literal`] 的定位/唯一性/容错语义，
//! 最后**只写一次**（要么整批落到磁盘，要么一个字节都不写）。
//!
//! 失败策略由 `on_miss` 决定：
//! - `stop`（默认）：任一步未命中就整批不写，但会把**每一条**的命中情况都报出来，
//!   一轮就能把 N 处偏差全改对；
//! - `continue`：能命中的照常落盘，未命中的逐条报告，等价于脚本里的 `in` 判断。

use super::edit_match::MatchTier;
use super::edit_recovery;
use super::edit_tool::{replace_literal, EditFailure, PreparedEdit};
use super::tool_error::{build_failed_tool_result, ToolErrorMeta};
use serde_json::{json, Value};

/// 单次调用的步骤上限：再多就说明该走脚本或分批，而不是一条 if 里塞 200 处替换。
pub(super) const MAX_BATCH_STEPS: usize = 64;

/// 一条替换步骤。字段已在校验阶段定性，这里只保留执行所需的最小信息。
#[derive(Debug, Clone)]
pub(super) struct BatchStep {
    pub(super) old_string: String,
    pub(super) new_string: String,
    pub(super) replace_all: bool,
}

/// 批量编辑的执行报告。
#[derive(Debug, Clone)]
pub(super) struct BatchReport {
    /// 逐条结果，直接进 `data.edits`。
    pub(super) steps: Value,
    pub(super) applied: usize,
    pub(super) missed: usize,
}

impl BatchReport {
    pub(super) fn is_partial(&self) -> bool {
        self.missed > 0
    }
}

/// 未命中导致的整批失败：带上逐条报告，模型一轮就能改对全部偏差。
#[derive(Debug)]
pub(super) struct BatchEditFailure {
    message: String,
    hint: String,
    data: Value,
}

impl std::fmt::Display for BatchEditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for BatchEditFailure {}

pub(super) fn batch_requested(args: &Value) -> bool {
    args.get("edits").is_some_and(|value| !value.is_null())
}

/// 未命中时是否继续落盘已命中的部分；默认整批回退。
pub(super) fn continue_on_miss(args: &Value) -> bool {
    matches!(
        args.get("on_miss").and_then(Value::as_str).map(str::trim),
        Some("continue")
    )
}

/// 取字符串字段，兼容常见别名；非字符串视为缺失。
fn text_field(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        item.get(*key)
            .and_then(Value::as_str)
            .map(str::to_string)
    })
}

/// 参数校验：`edits` 存在时与其它编辑形态互斥，且每条都必须能定位。
pub(super) fn validate_batch_args(args: &Value) -> Option<Value> {
    if !batch_requested(args) {
        return None;
    }
    let conflicts = [
        "old_string",
        "new_string",
        "start_line",
        "end_line",
        "command",
        "file_text",
        "input",
        "patch",
    ]
    .into_iter()
    .filter(|key| args.get(*key).is_some())
    .collect::<Vec<_>>();
    if !conflicts.is_empty() {
        return Some(invalid(
            "edits 不能与 old_string/new_string、行区间、子命令或补丁同时使用",
            json!({ "fields": conflicts }),
            "批量替换只给 file_path + edits；单条替换才用 old_string/new_string。",
        ));
    }
    let Some(items) = args.get("edits").and_then(Value::as_array) else {
        return Some(invalid(
            "edits 必须是数组",
            json!({ "edits_type": "not_array" }),
            "每条形如 {\"old_string\": \"...\", \"new_string\": \"...\"}。",
        ));
    };
    if items.is_empty() {
        return Some(invalid(
            "edits 不能是空数组",
            json!({ "edits_len": 0 }),
            "没有要改的内容就不要调用编辑工具。",
        ));
    }
    if items.len() > MAX_BATCH_STEPS {
        return Some(invalid(
            &format!("edits 最多 {MAX_BATCH_STEPS} 条，当前 {}", items.len()),
            json!({ "edits_len": items.len(), "max": MAX_BATCH_STEPS }),
            "拆成多次调用，或改用命令工具跑脚本批量处理。",
        ));
    }
    for (index, item) in items.iter().enumerate() {
        if !item.is_object() {
            return Some(invalid(
                &format!("第 {} 条 edit 不是对象", index + 1),
                json!({ "step": index + 1 }),
                "每条形如 {\"old_string\": \"...\", \"new_string\": \"...\"}。",
            ));
        }
        let old = text_field(item, &["old_string", "old_str", "old_text"])
            .filter(|value| !value.is_empty());
        if old.is_none() {
            return Some(invalid(
                &format!("第 {} 条 edit 缺少非空 old_string", index + 1),
                json!({ "step": index + 1, "field": "old_string" }),
                "old_string 是被替换掉的原文；要删除内容时 new_string 留空即可。",
            ));
        }
        if text_field(item, &["new_string", "new_str", "new_text"]).is_none() {
            return Some(invalid(
                &format!("第 {} 条 edit 缺少 new_string", index + 1),
                json!({ "step": index + 1, "field": "new_string" }),
                "必须显式给出字符串 new_string；空字符串表示删除该片段。",
            ));
        }
    }
    if let Some(value) = args.get("on_miss") {
        let ok = matches!(
            value.as_str().map(str::trim),
            Some("stop") | Some("continue")
        );
        if !ok {
            return Some(invalid(
                "on_miss 只能是 stop 或 continue",
                json!({ "on_miss": value }),
                "stop 表示任一条未命中就整批不写；continue 表示先落已命中的部分。",
            ));
        }
    }
    None
}

fn invalid(message: &str, data: Value, hint: &str) -> Value {
    build_failed_tool_result(
        message,
        data,
        ToolErrorMeta::new("TOOL_EDIT_INVALID_ARGS", Some(hint.to_string()), false, None),
        false,
    )
}

/// 解析已通过校验的 `edits`；字段别名在这里归一。
pub(super) fn parse_batch(args: &Value) -> Option<Vec<BatchStep>> {
    let items = args.get("edits")?.as_array()?;
    let mut steps = Vec::with_capacity(items.len());
    for item in items {
        steps.push(BatchStep {
            old_string: text_field(item, &["old_string", "old_str", "old_text"])?,
            new_string: text_field(item, &["new_string", "new_str", "new_text"]).unwrap_or_default(),
            replace_all: item
                .get("replace_all")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Some(steps)
}

/// 在同一份内容上按顺序套用所有步骤；每一步都用与单条替换相同的定位语义。
pub(super) fn prepare(
    content: &str,
    steps: &[BatchStep],
    continue_on_miss: bool,
) -> Result<(PreparedEdit, BatchReport), BatchEditFailure> {
    let mut working = content.to_string();
    let mut reports = Vec::with_capacity(steps.len());
    let mut applied = 0usize;
    let mut missed = 0usize;
    let mut replacements = 0usize;
    let mut tier = MatchTier::Exact;

    for (index, step) in steps.iter().enumerate() {
        match replace_literal(&working, &step.old_string, &step.new_string, step.replace_all) {
            Ok((next, found)) => {
                applied += 1;
                replacements += found.occurrences();
                tier = tier.max(found.tier);
                reports.push(json!({
                    "index": index + 1,
                    "status": "applied",
                    "replacements": found.occurrences(),
                    "match": found.tier.as_str(),
                }));
                working = next;
            }
            Err(failure) => {
                missed += 1;
                reports.push(step_failure_json(index, &failure, &working));
            }
        }
    }

    if missed > 0 && !continue_on_miss {
        // 失败结果在观察层会被压缩到「只剩 error 文本」，所以未命中的定位
        // 要点必须写进 message 本身，`data.edits` 只是给未压缩通路的备份。
        let message = format!(
            "批量编辑未命中 {missed}/{} 条，文件未改动：{}",
            steps.len(),
            miss_summary(&reports)
        );
        return Err(BatchEditFailure {
            message,
            hint: miss_hint(&reports),
            data: json!({
                "applied": applied,
                "missed": missed,
                "edits": reports,
            }),
        });
    }

    let changed = working != content;
    let diff = if changed {
        super::line_diff::diff_replaced(content, &working)
    } else {
        super::line_diff::LineDiff::default()
    };
    let report = BatchReport {
        steps: Value::Array(reports),
        applied,
        missed,
    };
    Ok((
        PreparedEdit {
            updated: working,
            occurrences: replacements,
            diff,
            unavailable: None,
            changed,
            tier,
        },
        report,
    ))
}

/// 单条失败的结构化报告：`nearest` 让人不必回读文件就能改对。
///
/// 批量里每条只留**最相似的那一个**片段：一份 8 条的调用若每条都带 3 段预览，
/// 结果会膨胀成几 KB，而定位只需要最近的一处。
fn step_failure_json(index: usize, failure: &EditFailure, content: &str) -> Value {
    match failure {
        EditFailure::NoMatch { nearest } => json!({
            "index": index + 1,
            "status": "missed",
            "reason": "no_match",
            "nearest": edit_recovery::to_json(&nearest[..nearest.len().min(1)]),
        }),
        EditFailure::NotUnique { count, lines } => json!({
            "index": index + 1,
            "status": "missed",
            "reason": "not_unique",
            "matches": count,
            "lines": lines,
        }),
        EditFailure::NotFound => json!({
            "index": index + 1,
            "status": "missed",
            "reason": "not_found",
            "file_lines": content.lines().count(),
        }),
    }
}

/// 未命中清单（写进错误正文）：每条给出序号、原因与定位要点，最多 8 条。
///
/// 失败结果在观察层会被压缩成「只剩 error 文本」，这份清单是模型实际能看到的
/// 诊断，必须自解释：不需要再读 `data`、也不需要重跑一遍才知道哪条错了。
fn miss_summary(reports: &[Value]) -> String {
    let mut items = Vec::new();
    for report in reports {
        if report.get("status").and_then(Value::as_str) != Some("missed") {
            continue;
        }
        if items.len() >= 8 {
            items.push("……（其余未逐条列出）".to_string());
            break;
        }
        let index = report.get("index").and_then(Value::as_u64).unwrap_or(0);
        let detail = match report.get("reason").and_then(Value::as_str) {
            Some("not_unique") => {
                let count = report
                    .get("matches")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let first_line = report
                    .get("lines")
                    .and_then(Value::as_array)
                    .and_then(|lines| lines.first())
                    .and_then(Value::as_u64);
                match first_line {
                    Some(line) => format!("第 {index} 条匹配到 {count} 处（首个在第 {line} 行）"),
                    None => format!("第 {index} 条匹配到 {count} 处"),
                }
            }
            Some("not_found") => format!("第 {index} 条目标文件已不存在"),
            _ => {
                let nearest = report
                    .get("nearest")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first());
                let line = nearest
                    .and_then(|first| first.get("line"))
                    .and_then(Value::as_u64);
                let similarity = nearest
                    .and_then(|first| first.get("similarity"))
                    .and_then(Value::as_u64);
                match (line, similarity) {
                    (Some(line), Some(similarity)) => format!(
                        "第 {index} 条未匹配（最近片段在第 {line} 行，相似度 {similarity}%）"
                    ),
                    (Some(line), None) => {
                        format!("第 {index} 条未匹配（最近片段在第 {line} 行）")
                    }
                    _ => format!("第 {index} 条未匹配"),
                }
            }
        };
        items.push(detail);
    }
    items.join("；")
}

/// 失败提示：直接指向第一个未命中步骤的最相近片段，省掉一轮 grep。
fn miss_hint(reports: &[Value]) -> String {
    for report in reports {
        if report.get("status").and_then(Value::as_str) != Some("missed") {
            continue;
        }
        let index = report.get("index").and_then(Value::as_u64).unwrap_or(0);
        match report.get("reason").and_then(Value::as_str) {
            Some("not_unique") => {
                let count = report.get("matches").and_then(Value::as_u64).unwrap_or(0);
                return format!(
                    "第 {index} 条匹配到 {count} 处：补足上下文使其唯一，或在该条上设 replace_all=true。"
                );
            }
            _ => {
                if let Some(line) = report
                    .get("nearest")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|first| first.get("line"))
                    .and_then(Value::as_u64)
                {
                    return format!(
                        "第 {index} 条按 nearest[0] 校正：该片段现在从第 {line} 行开始；空白差异已自动容错，报错说明内容本身不同。改对这一条后整批重发即可（本次未写入任何内容）。"
                    );
                }
                return format!(
                    "第 {index} 条未命中：按 nearest 的逐行差异校正后整批重发；或设 on_miss=continue 先落已匹配的部分。"
                );
            }
        }
    }
    "按 steps 里的未命中项校正后重试；本次未写入任何内容。".to_string()
}

/// 未命中失败的最终载荷。
pub(super) fn failure_result(path: &str, dry_run: bool, failure: &BatchEditFailure) -> Value {
    let mut data = match failure.data.clone() {
        Value::Object(map) => map,
        other => return other,
    };
    data.insert("path".to_string(), json!(path));
    data.insert("dry_run".to_string(), json!(dry_run));
    build_failed_tool_result(
        failure.message.clone(),
        Value::Object(data),
        ToolErrorMeta::new(
            "TOOL_EDIT_BATCH_MISSED",
            Some(failure.hint.clone()),
            false,
            None,
        ),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(raw: Value) -> Vec<BatchStep> {
        parse_batch(&json!({ "edits": raw })).expect("valid steps")
    }

    #[test]
    fn applies_every_step_in_one_pass() {
        let content = "a = 1;\nb = 2;\nc = 3;\n";
        let steps = steps(json!([
            { "old_string": "a = 1;", "new_string": "a = 10;" },
            { "old_string": "c = 3;", "new_string": "c = 30;" }
        ]));
        let (edit, report) = prepare(content, &steps, false).expect("batch applies");
        assert_eq!(edit.updated, "a = 10;\nb = 2;\nc = 30;\n");
        assert_eq!(report.applied, 2);
        assert_eq!(report.missed, 0);
        assert!(edit.changed);
    }

    #[test]
    fn stop_mode_reports_every_miss_without_writing() {
        let content = "a = 1;\nb = 2;\n";
        let steps = steps(json!([
            { "old_string": "a = 9;", "new_string": "a = 10;" },
            { "old_string": "b = 2;", "new_string": "b = 20;" },
            { "old_string": "zzz;", "new_string": "yyy;" }
        ]));
        let failure = prepare(content, &steps, false).expect_err("two misses");
        assert!(failure.message.contains("2/3"));
        // 未命中的定位要点必须在正文里：失败结果在观察层会被压成只剩 error 文本。
        assert!(failure.message.contains("第 1 条未匹配"), "{}", failure.message);
        assert!(failure.message.contains("第 3 条未匹配"), "{}", failure.message);
        let reports = failure.data.get("edits").expect("report list");
        assert_eq!(reports[0]["status"], json!("missed"));
        assert_eq!(reports[1]["status"], json!("applied"));
        assert_eq!(reports[2]["status"], json!("missed"));
    }

    #[test]
    fn continue_mode_keeps_applied_steps() {
        let content = "a = 1;\nb = 2;\n";
        let steps = steps(json!([
            { "old_string": "a = 9;", "new_string": "a = 10;" },
            { "old_string": "b = 2;", "new_string": "b = 20;" }
        ]));
        let (edit, report) = prepare(content, &steps, true).expect("partial apply");
        assert_eq!(edit.updated, "a = 1;\nb = 20;\n");
        assert_eq!(report.applied, 1);
        assert_eq!(report.missed, 1);
        assert!(report.is_partial());
    }

    #[test]
    fn steps_see_the_result_of_earlier_steps() {
        let content = "value = 1;\n";
        let steps = steps(json!([
            { "old_string": "value = 1;", "new_string": "value = 2;" },
            { "old_string": "value = 2;", "new_string": "value = 3;" }
        ]));
        let (edit, _) = prepare(content, &steps, false).expect("chained");
        assert_eq!(edit.updated, "value = 3;\n");
    }

    #[test]
    fn whitespace_drift_is_tolerated_per_step() {
        let content = "fn main() {\n    run();\n}\n";
        let steps = steps(json!([
            { "old_string": "fn main() {\n  run();\n}", "new_string": "fn main() {\n    stop();\n}" }
        ]));
        let (edit, report) = prepare(content, &steps, false).expect("tolerant");
        assert_eq!(edit.updated, "fn main() {\n    stop();\n}\n");
        assert_eq!(report.applied, 1);
        assert!(!edit.tier.is_exact());
    }
}
