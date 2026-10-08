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
struct QuestionPanelRoute {
    label: String,
    description: Option<String>,
    recommended: bool,
}

#[derive(Debug)]
struct QuestionPanelPayload {
    question: String,
    routes: Vec<QuestionPanelRoute>,
    multiple: bool,
}

pub(crate) async fn execute_question_panel_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload = normalize_question_panel_payload(args)?;
    let question = payload.question.clone();
    let routes = payload
        .routes
        .iter()
        .map(|route| {
            json!({
                "label": route.label,
                "description": route.description,
                "recommended": route.recommended
            })
        })
        .collect::<Vec<_>>();
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "question_panel",
            json!({
                "question": question.clone(),
                "routes": routes.clone(),
                "multiple": payload.multiple,
                "keep_open": true
            }),
        );
    }
    Ok(build_model_tool_success(
        "question_panel",
        "awaiting_input",
        "Opened a question panel and is waiting for user input.",
        json!({
            "question": question,
            "routes": routes,
            "multiple": payload.multiple
        }),
    ))
}

fn normalize_question_panel_payload(args: &Value) -> Result<QuestionPanelPayload> {
    let Some(obj) = args.as_object() else {
        return Err(anyhow!(i18n::t("tool.question_panel.routes_required")));
    };
    let question = obj
        .get("question")
        .or_else(|| obj.get("prompt"))
        .or_else(|| obj.get("title"))
        .or_else(|| obj.get("header"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let question = if question.is_empty() {
        i18n::t("tool.question_panel.default_question")
    } else {
        question
    };
    let multiple = obj
        .get("multiple")
        .or_else(|| obj.get("allow_multiple"))
        .or_else(|| obj.get("multi"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let routes = obj
        .get("routes")
        .or_else(|| obj.get("options"))
        .or_else(|| obj.get("choices"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut normalized = Vec::new();
    for item in routes {
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
        normalized.push(QuestionPanelRoute {
            label,
            description,
            recommended,
        });
    }
    if normalized.is_empty() {
        return Err(anyhow!(i18n::t("tool.question_panel.routes_required")));
    }
    Ok(QuestionPanelPayload {
        question,
        routes: normalized,
        multiple,
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_plan_status;

    #[test]
    fn normalize_plan_status_accepts_expected_aliases() {
        assert_eq!(normalize_plan_status(Some("in-progress")), "in_progress");
        assert_eq!(normalize_plan_status(Some("done")), "completed");
        assert_eq!(normalize_plan_status(Some("unknown")), "pending");
    }
}
