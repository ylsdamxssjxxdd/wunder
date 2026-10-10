use super::{build_model_tool_success, context::ToolContext};
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

pub(crate) async fn execute_question_panel_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let questions = normalize_question_panel_questions(args)?;
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
    Ok(build_model_tool_success(
        "question_panel",
        "awaiting_input",
        "Opened a question panel and is waiting for user input.",
        json!({ "questions": questions_json }),
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

fn normalize_question_panel_questions(args: &Value) -> Result<Vec<QuestionPanelQuestion>> {
    let Some(obj) = args.as_object() else {
        return Err(anyhow!(i18n::t("tool.question_panel.options_required")));
    };
    let raw_items = match obj.get("questions").and_then(Value::as_array) {
        Some(items) => items.clone(),
        None => {
            // 单题扁平写法：历史线程与仍在用旧契约的调用方要能继续渲染。
            vec![json!({
                "question": first_panel_value(obj, &["question", "prompt", "title", "header"]),
                "options": first_panel_value(obj, &["options", "routes", "choices"]),
                "multiple": first_panel_value(obj, &["multiple", "allow_multiple", "multi"]),
            })]
        }
    };
    let mut questions = Vec::new();
    for item in raw_items.into_iter().take(MAX_QUESTION_PANEL_QUESTIONS) {
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
        let options = normalize_question_panel_options(
            record
                .get("options")
                .or_else(|| record.get("routes"))
                .or_else(|| record.get("choices")),
        );
        if options.is_empty() {
            continue;
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
        return Err(anyhow!(i18n::t("tool.question_panel.options_required")));
    }
    Ok(questions)
}

fn first_panel_value(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Value {
    keys.iter()
        .find_map(|key| obj.get(*key))
        .cloned()
        .unwrap_or(Value::Null)
}

fn normalize_question_panel_options(value: Option<&Value>) -> Vec<QuestionPanelOption> {
    let items = value.and_then(Value::as_array).cloned().unwrap_or_default();
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
        .expect("questions");
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
        .expect("legacy questions");
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
        .expect("kept questions");
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].question, "有选项的一题");

        let err = normalize_question_panel_questions(&json!({ "questions": [] }))
            .expect_err("no usable question");
        assert!(err.to_string().contains("选项"));
    }
}
