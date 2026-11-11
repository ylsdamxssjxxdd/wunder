use super::{build_model_tool_success, build_model_tool_success_with_hint, context::ToolContext};
use crate::i18n;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct PlanUpdateArgs {
    #[serde(default)]
    explanation: Option<String>,
    plan: Vec<PlanItemArgs>,
}

#[derive(Debug, Deserialize)]
struct PlanItemArgs {
    step: String,
    #[serde(default)]
    status: Option<String>,
}

pub(crate) async fn execute_plan_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: PlanUpdateArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    if payload.plan.is_empty() {
        return Err(anyhow!(i18n::t("tool.plan.plan_required")));
    }
    let mut seen_in_progress = false;
    let mut normalized_plan = Vec::new();
    for item in payload.plan {
        let step = item.step.trim().to_string();
        if step.is_empty() {
            continue;
        }
        let mut status = normalize_plan_status(item.status.as_deref());
        if status == "in_progress" {
            if seen_in_progress {
                status = "pending".to_string();
            } else {
                seen_in_progress = true;
            }
        }
        normalized_plan.push(json!({
            "step": step,
            "status": status
        }));
    }
    if normalized_plan.is_empty() {
        return Err(anyhow!(i18n::t("tool.plan.plan_required")));
    }
    let explanation = payload.explanation.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "plan_update",
            json!({
                "explanation": explanation,
                "plan": normalized_plan
            }),
        );
    }
    Ok(build_model_tool_success(
        "plan_update",
        "completed",
        "Updated the execution plan.",
        json!({ "status": "ok" }),
    ))
}

fn normalize_plan_status(value: Option<&str>) -> String {
    let raw = value.unwrap_or("").trim().to_lowercase();
    if raw.is_empty() {
        return "pending".to_string();
    }
    let normalized = raw.replace(['-', ' '], "_");
    match normalized.as_str() {
        "pending" => "pending".to_string(),
        "in_progress" | "inprogress" => "in_progress".to_string(),
        "completed" | "complete" | "done" => "completed".to_string(),
        _ => "pending".to_string(),
    }
}

#[derive(Debug)]
struct QuestionPanelOption {
    label: String,
    description: Option<String>,
    recommended: bool,
}

#[derive(Debug)]
struct QuestionPanelQuestion {
    question: String,
    options: Vec<QuestionPanelOption>,
    multiple: bool,
}

/// 面板一次最多分页展示的问题数，与工具 schema 的 maxItems 对齐。
const MAX_QUESTION_PANEL_QUESTIONS: usize = 4;
/// 单题最多展示的选项数，与工具 schema 的 options.maxItems 对齐。
const MAX_QUESTION_PANEL_OPTIONS: usize = 4;
/// 顶层写成单题（question/options 平铺）时的兼容字段。
const QUESTION_PANEL_FLAT_QUESTION_KEYS: &[&str] = &["question", "prompt", "title", "header"];
const QUESTION_PANEL_FLAT_OPTION_KEYS: &[&str] = &["options", "routes", "choices"];

pub(crate) async fn execute_question_panel_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let parsed = normalize_question_panel_questions(args)?;
    let questions = parsed.questions;
    let questions_json = questions
        .iter()
        .map(question_panel_question_json)
        .collect::<Vec<_>>();
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "question_panel",
            json!({
                "questions": questions_json.clone(),
                "keep_open": true
            }),
        );
    }
    // 被丢掉或截断的题不能静默消失：提示里说清是哪一题、缺了什么或多在哪，
    // 模型下一轮才能补上。
    let hint = if parsed.dropped.is_empty() {
        None
    } else {
        let mut hint = String::from("Question panel: these items were not shown: ");
        hint.push_str(&parsed.dropped.join("; "));
        Some(hint)
    };
    Ok(build_model_tool_success_with_hint(
        "question_panel",
        "awaiting_input",
        "Opened a question panel and is waiting for user input.",
        json!({
            "questions": questions_json,
            "dropped_questions": parsed.dropped,
        }),
        hint,
    ))
}

fn question_panel_question_json(question: &QuestionPanelQuestion) -> Value {
    json!({
        "question": question.question,
        "multiple": question.multiple,
        "options": question.options.iter().map(|option| json!({
            "label": option.label,
            "description": option.description,
            "recommended": option.recommended
        })).collect::<Vec<_>>()
    })
}

#[derive(Debug)]
struct QuestionPanelParse {
    questions: Vec<QuestionPanelQuestion>,
    dropped: Vec<String>,
}

fn normalize_question_panel_questions(args: &Value) -> Result<QuestionPanelParse> {
    let Some(obj) = args.as_object() else {
        return Err(question_panel_shape_error(format!(
            "没有收到参数对象，实际是 {}",
            value_kind(args)
        )));
    };
    let raw_items: Vec<Value> = match obj.get("questions") {
        // questions 存在但不是数组时不要退化成单题：那会把错误指向合成出来的
        // 字段，让调用方以为自己的 options 写错了。
        Some(value) if !value.is_array() => {
            return Err(question_panel_shape_error(format!(
                "questions 必须是数组，实际是 {}",
                value_kind(value)
            )));
        }
        Some(value) => value.as_array().cloned().unwrap_or_default(),
        None => {
            let has_flat = QUESTION_PANEL_FLAT_QUESTION_KEYS
                .iter()
                .chain(QUESTION_PANEL_FLAT_OPTION_KEYS.iter())
                .any(|key| obj.contains_key(*key));
            if !has_flat {
                let keys = obj.keys().cloned().collect::<Vec<_>>().join("、");
                let detail = if keys.is_empty() {
                    "缺少 questions 数组，实际参数是空对象".to_string()
                } else {
                    format!("缺少 questions 数组，实际顶层字段有 {keys}")
                };
                return Err(question_panel_shape_error(detail));
            }
            // 单题扁平写法：历史线程与仍在用旧契约的调用方要能继续渲染。
            vec![json!({
                "question": first_panel_value(obj, QUESTION_PANEL_FLAT_QUESTION_KEYS),
                "options": first_panel_value(obj, QUESTION_PANEL_FLAT_OPTION_KEYS),
                "multiple": first_panel_value(obj, &["multiple", "allow_multiple", "multi"]),
            })]
        }
    };
    if raw_items.is_empty() {
        return Err(question_panel_shape_error(
            "questions 是空数组，至少要给出 1 题".to_string(),
        ));
    }
    let checked = raw_items
        .iter()
        .take(MAX_QUESTION_PANEL_QUESTIONS)
        .collect::<Vec<_>>();
    // 没展示出来的内容一律点名：被丢掉的题或选项不能静默消失。
    let mut dropped = question_panel_problems(&checked);
    if raw_items.len() > MAX_QUESTION_PANEL_QUESTIONS {
        let from = MAX_QUESTION_PANEL_QUESTIONS + 1;
        let to = raw_items.len();
        let position = if from == to {
            format!("第 {from} 题")
        } else {
            format!("第 {}–{} 题", from, to)
        };
        dropped.push(format!(
            "{position}超出单次上限 {} 题，未展示",
            MAX_QUESTION_PANEL_QUESTIONS
        ));
    }
    let mut questions = Vec::new();
    for (index, item) in checked.iter().enumerate() {
        let Value::Object(record) = item else {
            continue;
        };
        let question = record
            .get("question")
            .or_else(|| record.get("title"))
            .or_else(|| record.get("header"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let question = if question.is_empty() {
            i18n::t("tool.question_panel.default_question")
        } else {
            question
        };
        let raw_options = record
            .get("options")
            .or_else(|| record.get("routes"))
            .or_else(|| record.get("choices"));
        let option_total = raw_options.map(option_item_count).unwrap_or(0);
        let options = normalize_question_panel_options(raw_options);
        if options.is_empty() {
            continue;
        }
        if option_total > MAX_QUESTION_PANEL_OPTIONS {
            dropped.push(format!(
                "第 {} 题有 {} 个选项，超过上限 {} 个，仅展示前 {} 个",
                index + 1,
                option_total,
                MAX_QUESTION_PANEL_OPTIONS,
                MAX_QUESTION_PANEL_OPTIONS
            ));
        }
        let multiple = record
            .get("multiple")
            .or_else(|| record.get("allow_multiple"))
            .or_else(|| record.get("multi"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        questions.push(QuestionPanelQuestion {
            question,
            options,
            multiple,
        });
    }
    if questions.is_empty() {
        return Err(question_panel_options_error(&checked, args));
    }
    Ok(QuestionPanelParse { questions, dropped })
}

/// 选项缺失时不要只回一句「每个问题至少要给出一个选项」：把解析到什么、第几题
/// 缺字段、正确形状一起回给模型，才能一次改对而不是反复猜。
fn question_panel_options_error(checked: &[&Value], args: &Value) -> anyhow::Error {
    let mut details = question_panel_problems(checked);
    if details.is_empty() {
        details.push(match args.as_object() {
            Some(obj) if obj.is_empty() => "没有可渲染的问题项，实际参数是空对象".to_string(),
            Some(obj) => format!(
                "没有可渲染的问题项，实际顶层字段有 {}",
                obj.keys().cloned().collect::<Vec<_>>().join("、")
            ),
            None => format!(
                "没有可渲染的问题项，参数实际是 {}",
                value_kind(args)
            ),
        });
    }
    question_panel_shape_error_with(details)
}

fn question_panel_shape_error(detail: String) -> anyhow::Error {
    // 结构性问题（没收到 questions）不该套「每个问题至少要给出一个选项」，
    // 那句话会把人往错误的方向带。
    anyhow!(
        "问询面板参数不可用：{}。正确形状：{}。",
        detail,
        QUESTION_PANEL_SHAPE_HINT
    )
}

const QUESTION_PANEL_SHAPE_HINT: &str = "questions 数组，每项含 question 与 options（options 至少 1 项，可写字符串数组，也可写用 | 或 ，分隔的一整串）";

fn question_panel_shape_error_with(details: Vec<String>) -> anyhow::Error {
    // 文案自带句号，先剥掉再补，避免出现「选项。。」这种双句号。
    let mut message = i18n::t("tool.question_panel.options_required")
        .trim()
        .trim_end_matches(|c| c == '。' || c == '.')
        .to_string();
    message.push('。');
    if !details.is_empty() {
        message.push_str(&details.join("；"));
        message.push('。');
    }
    message.push_str("正确形状：");
    message.push_str(QUESTION_PANEL_SHAPE_HINT);
    message.push('。');
    anyhow!(message)
}

/// 逐项说明「第几题、什么问题、实际有什么字段」，供错误与丢弃提示共用。
fn question_panel_problems(items: &[&Value]) -> Vec<String> {
    let mut details = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let position = index + 1;
        match item {
            Value::Object(record) => {
                let option_count = record
                    .get("options")
                    .or_else(|| record.get("routes"))
                    .or_else(|| record.get("choices"))
                    .map(option_item_count)
                    .unwrap_or(0);
                if option_count > 0 {
                    continue;
                }
                let question = record
                    .get("question")
                    .or_else(|| record.get("title"))
                    .or_else(|| record.get("header"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                let mut line = String::from("第 ");
                line.push_str(&position.to_string());
                line.push_str(" 题缺少 options");
                if !question.is_empty() {
                    line.push_str("（题目：");
                    line.push_str(question);
                    line.push('）');
                }
                let keys = record.keys().cloned().collect::<Vec<_>>();
                if !keys.is_empty() {
                    line.push_str("，实际字段有 ");
                    line.push_str(&keys.join("、"));
                }
                details.push(line);
            }
            other => {
                let mut line = String::from("第 ");
                line.push_str(&position.to_string());
                line.push_str(" 项不是对象（实际是 ");
                line.push_str(value_kind(other));
                line.push('）');
                details.push(line);
            }
        }
    }
    details
}

/// options 既可能是数组，也可能是 `甲|乙` 这样的整串；两种都算数。
fn option_item_count(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.len(),
        Value::String(text) => text
            .split(['|', '，'])
            .filter(|part| !part.trim().is_empty())
            .count(),
        _ => 0,
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "布尔值",
        Value::Number(_) => "数字",
        Value::String(_) => "字符串",
        Value::Array(_) => "数组",
        Value::Object(_) => "对象",
    }
}

fn first_panel_value(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| obj.get(*key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn normalize_question_panel_options(value: Option<&Value>) -> Vec<QuestionPanelOption> {
    // options 既接受对象/字符串数组，也接受用 | 或，分隔的一整串，少写一层嵌套。
    let items = match value {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(text)) => text
            .split(['|', '，'])
            .map(|part| Value::String(part.trim().to_string()))
            .collect(),
        _ => Vec::new(),
    };
    let mut normalized = Vec::new();
    for item in items {
        let (label, description, recommended) = match item {
            Value::String(value) => (value, None, false),
            Value::Object(map) => {
                let label = map
                    .get("label")
                    .or_else(|| map.get("title"))
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let description = map
                    .get("description")
                    .or_else(|| map.get("detail"))
                    .or_else(|| map.get("desc"))
                    .or_else(|| map.get("summary"))
                    .and_then(Value::as_str)
                    .map(|value| value.to_string());
                let recommended = map
                    .get("recommended")
                    .or_else(|| map.get("preferred"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                (label, description, recommended)
            }
            _ => (String::new(), None, false),
        };
        let label = label.trim().to_string();
        if label.is_empty() {
            continue;
        }
        let description = description.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let recommended = recommended || label.contains("推荐");
        normalized.push(QuestionPanelOption {
            label,
            description,
            recommended,
        });
    }
    // 超出上限的选项不进入面板：调用方已在 dropped 里被明确告知。
    normalized.truncate(MAX_QUESTION_PANEL_OPTIONS);
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_plan_status_accepts_expected_aliases() {
        assert_eq!(normalize_plan_status(Some("in-progress")), "in_progress");
        assert_eq!(normalize_plan_status(Some("done")), "completed");
        assert_eq!(normalize_plan_status(Some("unknown")), "pending");
    }

    #[test]
    fn question_panel_keeps_every_question() {
        let questions = normalize_question_panel_questions(&json!({
            "questions": [
                {
                    "question": "第一个问题",
                    "options": [{ "label": "甲" }, { "label": "乙", "recommended": true }]
                },
                {
                    "question": "第二个问题",
                    "multiple": true,
                    "options": ["丙", "丁"]
                }
            ]
        }))
        .expect("questions")
        .questions;
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].options.len(), 2);
        assert!(questions[0].options[1].recommended);
        assert!(!questions[0].multiple);
        assert!(questions[1].multiple);
        assert_eq!(questions[1].options[0].label, "丙");
    }

    #[test]
    fn question_panel_still_reads_legacy_flat_payload() {
        let questions = normalize_question_panel_questions(&json!({
            "question": "旧版单题",
            "routes": [{ "label": "继续（推荐）" }],
            "allow_multiple": false
        }))
        .expect("legacy questions")
        .questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "旧版单题");
        // 标题里写「推荐」也要被识别成推荐项。
        assert!(questions[0].options[0].recommended);
    }

    #[test]
    fn question_panel_drops_optionless_questions_and_fails_when_empty() {
        let questions = normalize_question_panel_questions(&json!({
            "questions": [
                { "question": "没有选项的一题", "options": [] },
                { "question": "有选项的一题", "options": ["甲"] }
            ]
        }))
        .expect("kept questions")
        .questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "有选项的一题");

        let err = normalize_question_panel_questions(&json!({ "questions": [] }))
            .expect_err("no usable question");
        assert!(err.to_string().contains("questions"), "{}", err);
    }

    #[test]
    fn question_panel_accepts_pipe_separated_options() {
        let questions = normalize_question_panel_questions(&json!({
            "questions": [{ "question": "选哪个方向", "options": "甲|乙" }]
        }))
        .expect("questions")
        .questions;
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].options.len(), 2);
        assert_eq!(questions[0].options[1].label, "乙");
    }

    #[test]
    fn question_panel_reports_dropped_questions_instead_of_hiding_them() {
        let parsed = normalize_question_panel_questions(&json!({
            "questions": [
                { "question": "有选项的一题", "options": ["甲"] },
                { "question": "没有选项的一题", "hint": "漏了 options" }
            ]
        }))
        .expect("one usable question");
        assert_eq!(parsed.questions.len(), 1);
        assert_eq!(parsed.dropped.len(), 1);
        assert!(
            parsed.dropped[0].contains("第 2 题缺少 options"),
            "{:?}",
            parsed.dropped
        );
        assert!(
            parsed.dropped[0].contains("没有选项的一题"),
            "{:?}",
            parsed.dropped
        );
    }

    #[test]
    fn question_panel_error_keeps_a_shape_hint() {
        let err = normalize_question_panel_questions(&json!({ "question": "无选项" }))
            .expect_err("no usable question");
        let text = err.to_string();
        assert!(text.contains("第 1 题缺少 options"), "{}", text);
        assert!(text.contains("正确形状"), "{}", text);
    }

    #[test]
    fn question_panel_error_never_doubles_the_sentence_period() {
        let err = normalize_question_panel_questions(&json!({ "questions": [] }))
            .expect_err("empty questions");
        let text = err.to_string();
        assert!(!text.contains("。。"), "{}", text);
        assert!(!text.contains(".."), "{}", text);
    }

    #[test]
    fn question_panel_error_points_at_the_missing_questions_array() {
        // 参数整体没解析出来时，报错必须指向真正的问题，而不是编造一题缺 options。
        let err = normalize_question_panel_questions(&json!({})).expect_err("empty args");
        let text = err.to_string();
        assert!(text.contains("缺少 questions 数组"), "{}", text);
        assert!(text.contains("空对象"), "{}", text);
        assert!(!text.contains("第 1 题"), "{}", text);

        let err = normalize_question_panel_questions(&json!({ "raw": "{oops" }))
            .expect_err("unparsed payload");
        let text = err.to_string();
        assert!(text.contains("缺少 questions 数组"), "{}", text);
        assert!(text.contains("raw"), "{}", text);

        let err = normalize_question_panel_questions(&json!("甲|乙")).expect_err("not an object");
        assert!(err.to_string().contains("字符串"), "{}", err);
    }

    #[test]
    fn question_panel_rejects_non_array_questions() {
        let err = normalize_question_panel_questions(&json!({ "questions": "要问什么？" }))
            .expect_err("questions is a string");
        let text = err.to_string();
        assert!(text.contains("必须是数组"), "{}", text);
    }

    #[test]
    fn question_panel_reports_overflow_instead_of_dropping_silently() {
        let parsed = normalize_question_panel_questions(&json!({
            "questions": [
                { "question": "一", "options": ["甲", "乙", "丙", "丁", "戊"] },
                { "question": "二", "options": ["甲"] },
                { "question": "三", "options": ["甲"] },
                { "question": "四", "options": ["甲"] },
                { "question": "五", "options": ["甲"] }
            ]
        }))
        .expect("kept questions");
        assert_eq!(parsed.questions.len(), MAX_QUESTION_PANEL_QUESTIONS);
        assert_eq!(parsed.questions[0].options.len(), MAX_QUESTION_PANEL_OPTIONS);
        assert!(
            parsed
                .dropped
                .iter()
                .any(|line| line.contains("第 5 题") && line.contains("超出单次上限")),
            "{:?}",
            parsed.dropped
        );
        assert!(
            parsed
                .dropped
                .iter()
                .any(|line| line.contains("第 1 题") && line.contains("5 个选项")),
            "{:?}",
            parsed.dropped
        );
    }
}
