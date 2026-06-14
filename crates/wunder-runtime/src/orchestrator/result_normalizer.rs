use super::*;

#[derive(Clone, Debug)]
struct BusinessFailure {
    code: Option<String>,
    message: String,
    source: &'static str,
}

impl Orchestrator {
    pub(super) fn normalize_tool_result_payload(
        &self,
        tool_name: &str,
        result: ToolResultPayload,
    ) -> ToolResultPayload {
        normalize_tool_result_payload(tool_name, result)
    }
}

pub(super) fn normalize_tool_result_payload(
    tool_name: &str,
    mut result: ToolResultPayload,
) -> ToolResultPayload {
    let transport_ok = result.ok;
    let business_failure = if transport_ok {
        detect_business_failure(tool_name, &result.data)
    } else {
        None
    };
    let business_ok = business_failure.is_none();
    let final_ok = transport_ok && business_ok;
    if !final_ok {
        result.ok = false;
    }

    if result.error.trim().is_empty() {
        if let Some(failure) = business_failure.as_ref() {
            result.error = failure.message.clone();
        } else if !final_ok {
            if let Some(detail) = extract_error_detail_from_data(&result.data) {
                result.error = detail;
            }
        }
    }

    let explicit_error_meta = extract_error_meta(&result.data);
    let explicit_error_code = explicit_error_meta
        .as_ref()
        .and_then(|meta| meta.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let preflight_code = extract_reject_preflight_code(result.meta.as_ref());
    let explicit_retryable = explicit_error_meta
        .as_ref()
        .and_then(|meta| meta.get("retryable"))
        .and_then(Value::as_bool);

    let inferred_error_code = business_failure
        .as_ref()
        .and_then(|failure| failure.code.clone())
        .or_else(|| {
            infer_error_code(
                tool_name,
                result.error.as_str(),
                &result.data,
                transport_ok,
                business_ok,
            )
        });
    let error_code = explicit_error_code
        .or(preflight_code)
        .or(inferred_error_code)
        .unwrap_or_default();
    let error_retryable = infer_retryable(
        explicit_retryable,
        error_code.as_str(),
        result.error.as_str(),
        transport_ok,
        final_ok,
    );

    let mut meta = merge_meta(result.meta.take());
    meta.insert(
        "normalized_transport_ok".to_string(),
        Value::Bool(transport_ok),
    );
    meta.insert(
        "normalized_business_ok".to_string(),
        Value::Bool(business_ok),
    );
    meta.insert("normalized_final_ok".to_string(), Value::Bool(final_ok));
    meta.insert("error_retryable".to_string(), Value::Bool(error_retryable));
    if !error_code.is_empty() {
        meta.insert("error_code".to_string(), Value::String(error_code));
    }
    if !result.error.trim().is_empty() {
        let detail_head = result.error.chars().take(240).collect::<String>();
        meta.insert("error_detail_head".to_string(), Value::String(detail_head));
    }
    if let Some(failure) = business_failure {
        meta.insert(
            "business_error_source".to_string(),
            Value::String(failure.source.to_string()),
        );
    }
    result.meta = if meta.is_empty() {
        None
    } else {
        Some(Value::Object(meta))
    };
    result
}

fn detect_business_failure(tool_name: &str, data: &Value) -> Option<BusinessFailure> {
    let map = data.as_object()?;

    if map.get("is_error").and_then(Value::as_bool) == Some(true) {
        let message = extract_error_detail_from_data(data)
            .unwrap_or_else(|| "tool returned business error".to_string());
        return Some(BusinessFailure {
            code: infer_business_error_code(tool_name, &message),
            message,
            source: "is_error",
        });
    }

    if let Some(structured) = map.get("structured_content").and_then(Value::as_object) {
        let structured_ok = structured.get("ok").and_then(Value::as_bool);
        let structured_error = structured
            .get("error")
            .map(value_to_string)
            .or_else(|| structured.get("message").map(value_to_string))
            .unwrap_or_default();
        if structured_ok == Some(false) || !structured_error.trim().is_empty() {
            let message = if structured_error.trim().is_empty() {
                "tool returned structured business failure".to_string()
            } else {
                structured_error.trim().to_string()
            };
            return Some(BusinessFailure {
                code: infer_business_error_code(tool_name, &message),
                message,
                source: "structured_content",
            });
        }
    }

    if map.get("ok").and_then(Value::as_bool) == Some(false) {
        let message = extract_error_detail_from_data(data)
            .unwrap_or_else(|| "nested tool result marked as failed".to_string());
        return Some(BusinessFailure {
            code: infer_business_error_code(tool_name, &message),
            message,
            source: "nested_ok",
        });
    }

    None
}

fn infer_business_error_code(tool_name: &str, message: &str) -> Option<String> {
    infer_error_code(tool_name, message, &Value::Null, true, false)
}

fn infer_error_code(
    tool_name: &str,
    message: &str,
    data: &Value,
    transport_ok: bool,
    business_ok: bool,
) -> Option<String> {
    let text = message.trim();
    let lower = text.to_ascii_lowercase();
    if lower.contains("you have an error in your sql syntax")
        || lower.contains("sql syntax")
        || lower.contains("near '")
    {
        return Some("SQL_SYNTAX_ERROR".to_string());
    }
    if lower.contains("function") && lower.contains("does not exist") {
        return Some("SQL_FUNCTION_NOT_FOUND".to_string());
    }
    if lower.contains("unknown column") {
        return Some("SQL_UNKNOWN_COLUMN".to_string());
    }
    if lower.contains("indentationerror")
        || lower.contains("syntaxerror")
        || lower.contains("unmatched ']")
    {
        return Some("PYTHON_SYNTAX_ERROR".to_string());
    }
    if lower.contains("no such file or directory: 'mysql'") {
        return Some("DEPENDENCY_MYSQL_MISSING".to_string());
    }
    if lower.contains("no such file or directory") && is_execute_command_tool_name(tool_name) {
        return Some("COMMAND_NOT_FOUND".to_string());
    }
    if contains_timeout_hint(message) {
        return Some("TOOL_TIMEOUT".to_string());
    }
    if let Some(meta) = extract_error_meta(data) {
        if let Some(code) = meta.get("code").and_then(Value::as_str) {
            let cleaned = code.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    if !transport_ok {
        return Some("TOOL_EXEC_FAILED".to_string());
    }
    if !business_ok {
        return Some("TOOL_BUSINESS_FAILED".to_string());
    }
    None
}

fn infer_retryable(
    explicit: Option<bool>,
    error_code: &str,
    message: &str,
    transport_ok: bool,
    final_ok: bool,
) -> bool {
    if final_ok {
        return false;
    }
    if let Some(flag) = explicit {
        return flag;
    }
    if matches!(
        error_code,
        "SQL_SYNTAX_ERROR"
            | "SQL_FUNCTION_NOT_FOUND"
            | "SQL_UNKNOWN_COLUMN"
            | "PYTHON_SYNTAX_ERROR"
            | "DEPENDENCY_MYSQL_MISSING"
            | "COMMAND_NOT_FOUND"
    ) || error_code.starts_with("PRECHECK_")
    {
        return false;
    }
    if error_code == "TOOL_TIMEOUT" {
        return true;
    }
    if !transport_ok {
        let lower = message.to_ascii_lowercase();
        if contains_timeout_hint(message)
            || lower.contains("temporarily")
            || lower.contains("try again")
        {
            return true;
        }
    }
    false
}

fn contains_timeout_hint(message: &str) -> bool {
    let lower = message.trim().to_ascii_lowercase();
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("time out")
        || message.contains("超时")
}

fn extract_error_meta(data: &Value) -> Option<&Map<String, Value>> {
    data.as_object()
        .and_then(|map| map.get("error_meta"))
        .and_then(Value::as_object)
}

fn extract_error_detail_from_data(data: &Value) -> Option<String> {
    let map = data.as_object()?;
    if let Some(text) = map.get("error").and_then(Value::as_str) {
        let cleaned = text.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    if let Some(text) = map.get("message").and_then(Value::as_str) {
        let cleaned = text.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    if let Some(structured) = map.get("structured_content").and_then(Value::as_object) {
        if let Some(text) = structured.get("error").map(value_to_string) {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
        if let Some(text) = structured.get("message").map(value_to_string) {
            let cleaned = text.trim();
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }
    if let Some(stderr) = map.get("stderr").and_then(Value::as_str) {
        let cleaned = stderr.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn merge_meta(meta: Option<Value>) -> Map<String, Value> {
    match meta {
        Some(Value::Object(map)) => map,
        Some(Value::Null) | None => Map::new(),
        Some(other) => {
            let mut map = Map::new();
            map.insert("value".to_string(), other);
            map
        }
    }
}

fn extract_reject_preflight_code(meta: Option<&Value>) -> Option<String> {
    let preflight = meta
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("preflight"))
        .and_then(Value::as_object)?;
    let status = preflight
        .get("status")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if status != "reject" {
        return None;
    }
    preflight
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn is_execute_command_tool_name(tool_name: &str) -> bool {
    let cleaned = tool_name.trim();
    cleaned == resolve_tool_name("execute_command")
        || cleaned.eq_ignore_ascii_case("execute_command")
}

#[cfg(test)]
mod tests {
    use super::{normalize_tool_result_payload, ToolResultPayload};
    use chrono::Utc;
    use serde_json::{json, Value};

    #[test]
    fn normalizes_structured_content_failure_to_final_error() {
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "structured_content": {
                    "ok": false,
                    "error": "You have an error in your SQL syntax near 'x'"
                }
            }),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let normalized = normalize_tool_result_payload("extra_mcp@db_query", payload);
        assert!(!normalized.ok);
        assert!(normalized.error.contains("SQL syntax") || normalized.error.contains("syntax"));
        let meta = normalized.meta.expect("meta");
        assert_eq!(
            meta.get("normalized_final_ok")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            false
        );
        assert_eq!(
            meta.get("error_code").and_then(Value::as_str).unwrap_or(""),
            "SQL_SYNTAX_ERROR"
        );
    }

    #[test]
    fn preflight_code_takes_priority_over_inferred_code() {
        let payload = ToolResultPayload {
            ok: false,
            data: json!({
                "error": "timeout while calling service"
            }),
            error: "timeout while calling service".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "preflight": {
                    "status": "reject",
                    "code": "PRECHECK_SHELL_BAD_HEREDOC"
                }
            })),
        };
        let normalized = normalize_tool_result_payload("execute_command", payload);
        let meta = normalized.meta.expect("meta");
        assert_eq!(
            meta.get("error_code").and_then(Value::as_str),
            Some("PRECHECK_SHELL_BAD_HEREDOC")
        );
        assert_eq!(
            meta.get("error_retryable").and_then(Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn explicit_retryable_override_is_respected() {
        let payload = ToolResultPayload {
            ok: false,
            data: json!({
                "error_meta": {
                    "code": "PRECHECK_SQL_PUNCTUATION_NORMALIZED",
                    "retryable": true
                }
            }),
            error: "tool returned temporary reject".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let normalized = normalize_tool_result_payload("extra_mcp@db_query", payload);
        let meta = normalized.meta.expect("meta");
        assert_eq!(
            meta.get("error_code").and_then(Value::as_str),
            Some("PRECHECK_SQL_PUNCTUATION_NORMALIZED")
        );
        assert_eq!(
            meta.get("error_retryable").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn successful_payload_with_only_stderr_does_not_backfill_error() {
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "stdout": "saved",
                "stderr": "UserWarning: Glyph missing from font(s)"
            }),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let normalized = normalize_tool_result_payload("ptc", payload);
        assert!(normalized.ok);
        assert!(normalized.error.trim().is_empty());
        let meta = normalized.meta.expect("meta");
        assert_eq!(
            meta.get("normalized_final_ok").and_then(Value::as_bool),
            Some(true)
        );
        assert!(meta.get("error_code").is_none());
    }

    #[test]
    fn successful_rewrite_preflight_does_not_promote_error_code() {
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "stdout": "ok"
            }),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "preflight": {
                    "status": "rewrite",
                    "code": "PRECHECK_PYTHON_INDENTATION_NORMALIZED",
                    "summary": "Auto-fixed before run: dedented common leading indentation.",
                    "changes": ["dedented common leading indentation"]
                }
            })),
        };
        let normalized = normalize_tool_result_payload("ptc", payload);
        assert!(normalized.ok);
        let meta = normalized.meta.expect("meta");
        assert_eq!(
            meta.get("normalized_final_ok").and_then(Value::as_bool),
            Some(true)
        );
        assert!(meta.get("error_code").is_none());
        assert_eq!(
            meta.get("preflight")
                .and_then(Value::as_object)
                .and_then(|preflight| preflight.get("summary"))
                .and_then(Value::as_str),
            Some("Auto-fixed before run: dedented common leading indentation.")
        );
    }

    #[test]
    fn chinese_timeout_message_normalizes_to_retryable_timeout() {
        let payload = ToolResultPayload {
            ok: false,
            data: json!({
                "error": "网页抓取请求超时"
            }),
            error: "网页抓取请求超时".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let normalized = normalize_tool_result_payload("web_fetch", payload);
        let meta = normalized.meta.expect("meta");
        assert_eq!(
            meta.get("error_code").and_then(Value::as_str),
            Some("TOOL_TIMEOUT")
        );
        assert_eq!(
            meta.get("error_retryable").and_then(Value::as_bool),
            Some(true)
        );
    }
}
