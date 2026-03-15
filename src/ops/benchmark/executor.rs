use super::models::{AttemptUsage, ExecutionCapture, ToolCallRecord, ToolResultRecord};
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio_stream::StreamExt;

pub async fn execute_prompt(
    orchestrator: Arc<Orchestrator>,
    monitor: Arc<MonitorState>,
    request: WunderRequest,
    cancel_flag: Arc<AtomicBool>,
    session_id: &str,
) -> Result<ExecutionCapture, String> {
    let mut stream = orchestrator
        .stream(request)
        .await
        .map_err(|err| err.to_string())?;

    let mut transcript = Vec::new();
    let mut final_answer = String::new();
    let mut last_output = String::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut usage = AttemptUsage::default();
    let mut error_code = String::new();
    let mut error_message = String::new();
    let mut error_detail = Value::Null;

    while let Some(event) = stream.next().await {
        let event = event.map_err(|err| err.to_string())?;
        if cancel_flag.load(Ordering::SeqCst) {
            monitor.cancel(session_id);
        }
        let payload = event.data.get("data").cloned().unwrap_or(Value::Null);
        transcript.push(json!({
            "type": event.event,
            "payload": payload.clone(),
        }));

        match event.event.as_str() {
            "llm_output" => {
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    last_output = content.to_string();
                }
            }
            "tool_call" => {
                let name = payload
                    .get("tool")
                    .or_else(|| payload.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !name.is_empty() {
                    tool_calls.push(ToolCallRecord {
                        name,
                        args: payload.get("args").cloned().unwrap_or(Value::Null),
                        timestamp: payload
                            .get("timestamp")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0),
                    });
                }
            }
            "tool_result" => {
                let name = payload
                    .get("tool")
                    .or_else(|| payload.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                let preview = payload
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|text| text.chars().take(240).collect::<String>())
                    .unwrap_or_else(|| truncate_json_preview(&payload, 240));
                let ok = payload.get("error").is_none();
                tool_results.push(ToolResultRecord {
                    name,
                    ok,
                    preview,
                    timestamp: payload
                        .get("timestamp")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    raw: payload.clone(),
                });
            }
            "context_usage" => {
                usage.context_tokens = usage.context_tokens.max(
                    payload
                        .get("context_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                );
            }
            "round_usage" => {
                usage.request_count = usage.request_count.saturating_add(1);
                usage.input_tokens = usage.input_tokens.saturating_add(
                    payload
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                );
                usage.output_tokens = usage.output_tokens.saturating_add(
                    payload
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                );
                usage.total_tokens = usage.total_tokens.saturating_add(
                    payload
                        .get("total_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                );
            }
            "final" => {
                final_answer = payload
                    .get("answer")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if final_answer.is_empty() {
                    final_answer = last_output.trim().to_string();
                }
                if let Some(usage_payload) = payload.get("usage") {
                    usage.input_tokens = usage.input_tokens.max(
                        usage_payload
                            .get("input_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    );
                    usage.output_tokens = usage.output_tokens.max(
                        usage_payload
                            .get("output_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    );
                    usage.total_tokens = usage.total_tokens.max(
                        usage_payload
                            .get("total_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0),
                    );
                }
            }
            "error" => {
                error_code = payload
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                error_message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                error_detail = payload.get("detail").cloned().unwrap_or(Value::Null);
            }
            _ => {}
        }
    }

    if final_answer.is_empty() {
        final_answer = last_output.trim().to_string();
    }

    Ok(ExecutionCapture {
        transcript,
        final_answer,
        tool_calls,
        tool_results,
        usage,
        error_code,
        error_message,
        error_detail,
    })
}

fn truncate_json_preview(value: &Value, max_chars: usize) -> String {
    let text = value.to_string();
    if text.chars().count() <= max_chars {
        return text;
    }
    text.chars().take(max_chars).collect::<String>()
}
