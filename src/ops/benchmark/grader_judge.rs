use super::executor::execute_prompt;
use super::models::ExecutionCapture;
use super::spec::BenchmarkTaskSpec;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub async fn grade_with_judge(
    task: &BenchmarkTaskSpec,
    capture: &ExecutionCapture,
    orchestrator: Arc<Orchestrator>,
    monitor: Arc<MonitorState>,
    cancel_flag: Arc<AtomicBool>,
    user_id: &str,
    session_id: &str,
    judge_model_name: Option<String>,
    config_overrides: Option<Value>,
    language: Option<String>,
) -> Result<Value> {
    let rubric = task
        .llm_judge_rubric
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format_grading_criteria(&task.grading_criteria));
    let prompt = build_judge_prompt(task, capture, &rubric);
    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: prompt,
        tool_names: Vec::new(),
        skip_tool_calls: true,
        stream: true,
        debug_payload: false,
        session_id: Some(session_id.to_string()),
        agent_id: None,
        model_name: judge_model_name,
        language,
        config_overrides,
        agent_prompt: None,
        preview_skill: false,
        attachments: None,
        allow_queue: true,
        is_admin: false,
        approval_tx: None,
    };
    let result = execute_prompt(orchestrator, monitor, request, cancel_flag, session_id)
        .await
        .map_err(|err| anyhow!(err))?;
    let raw = parse_judge_response(&result.final_answer);
    let normalized = normalize_judge_response(raw);
    Ok(json!({
        "score": normalized.get("total").and_then(Value::as_f64).unwrap_or(0.0),
        "breakdown": normalized.get("scores").cloned().unwrap_or_else(|| json!({})),
        "notes": normalized.get("notes").and_then(Value::as_str).unwrap_or(""),
        "raw_response": result.final_answer,
        "error": "",
    }))
}

fn build_judge_prompt(
    task: &BenchmarkTaskSpec,
    capture: &ExecutionCapture,
    rubric: &str,
) -> String {
    format!(
        "You are a grading function. Your ONLY job is to output a single JSON object.\n\nCRITICAL RULES:\n- Do NOT use any tools\n- Do NOT create files or run commands\n- Do NOT write any prose outside the JSON\n- Respond with ONLY a JSON object\n\nBe a strict evaluator. Reserve 1.0 for genuinely excellent performance. An average acceptable completion should score around 0.6-0.7. Deduct points for unnecessary steps, verbose output, and inefficient tool usage.\n\n## Task\n{}\n\n## Expected Behavior\n{}\n\n## Agent Transcript (summarized)\n{}\n\n## Grading Rubric\n{}\n\nScore each criterion from 0.0 to 1.0.\n\nRespond with ONLY this JSON structure: {{\"scores\": {{\"criterion_name\": 0.0}}, \"total\": 0.0, \"notes\": \"brief justification\"}}",
        task.prompt,
        task.expected_behavior,
        summarize_capture(capture),
        rubric
    )
}

fn summarize_capture(capture: &ExecutionCapture) -> String {
    let mut lines = Vec::new();
    for call in capture.tool_calls.iter().take(12) {
        lines.push(format!(
            "ToolCall: {} {}",
            call.name,
            truncate_value(&call.args, 160)
        ));
    }
    for result in capture.tool_results.iter().take(12) {
        let preview = result.preview.chars().take(200).collect::<String>();
        lines.push(format!("ToolResult: {} -> {}", result.name, preview));
    }
    if !capture.final_answer.trim().is_empty() {
        lines.push(format!(
            "FinalAnswer: {}",
            capture.final_answer.chars().take(800).collect::<String>()
        ));
    }
    if !capture.error_message.trim().is_empty() {
        lines.push(format!("Error: {}", capture.error_message));
    }
    lines.push(format!(
        "Usage: context_tokens={}, total_tokens={}, requests={}",
        capture.usage.context_tokens, capture.usage.total_tokens, capture.usage.request_count
    ));
    lines.join("\n")
}

fn truncate_value(value: &Value, max_chars: usize) -> String {
    let text = value.to_string();
    if text.chars().count() <= max_chars {
        return text;
    }
    text.chars().take(max_chars).collect::<String>()
}

fn format_grading_criteria(criteria: &[String]) -> String {
    if criteria.is_empty() {
        return "- Complete the task accurately and efficiently".to_string();
    }
    criteria
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_judge_response(text: &str) -> Value {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Value::Null;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value;
    }
    if let Some(code_block) = extract_json_code_block(trimmed) {
        if let Ok(value) = serde_json::from_str::<Value>(&code_block) {
            return value;
        }
    }
    for candidate in extract_json_candidates(trimmed).into_iter().rev() {
        if let Ok(value) = serde_json::from_str::<Value>(&candidate) {
            return value;
        }
    }
    Value::Null
}

fn extract_json_code_block(text: &str) -> Option<String> {
    let regex = regex::Regex::new(r"(?s)```json\s*(.*?)\s*```").ok()?;
    regex
        .captures(text)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn extract_json_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (index, ch) in text.char_indices() {
        if ch == '{' {
            if depth == 0 {
                start = Some(index);
            }
            depth += 1;
        } else if ch == '}' {
            if depth == 0 {
                continue;
            }
            depth -= 1;
            if depth == 0 {
                if let Some(begin) = start.take() {
                    candidates.push(text[begin..=index].to_string());
                }
            }
        }
    }
    candidates
}

fn normalize_judge_response(value: Value) -> Value {
    let mut scores = json!({});
    let mut total = None;
    let mut notes = String::new();
    if let Some(map) = value.as_object() {
        if let Some(found_scores) = map.get("scores").and_then(Value::as_object) {
            scores = Value::Object(found_scores.clone());
        } else if let Some(found_scores) = map.get("criteria_scores").and_then(Value::as_object) {
            scores = Value::Object(found_scores.clone());
        }
        total = map
            .get("total")
            .and_then(Value::as_f64)
            .or_else(|| map.get("score").and_then(Value::as_f64));
        notes = map
            .get("notes")
            .or_else(|| map.get("justification"))
            .or_else(|| map.get("reasoning"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
    }
    if total.is_none() {
        if let Some(values) = scores.as_object() {
            let floats = values
                .values()
                .filter_map(Value::as_f64)
                .collect::<Vec<_>>();
            if !floats.is_empty() {
                total = Some(floats.iter().sum::<f64>() / floats.len() as f64);
            }
        }
    }
    json!({
        "scores": scores,
        "total": total.unwrap_or(0.0),
        "notes": notes,
    })
}
