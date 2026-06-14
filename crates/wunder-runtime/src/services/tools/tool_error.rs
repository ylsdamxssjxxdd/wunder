use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolErrorMeta {
    pub(crate) code: String,
    pub(crate) hint: Option<String>,
    pub(crate) retryable: bool,
    pub(crate) retry_after_ms: Option<u64>,
}

impl ToolErrorMeta {
    pub(crate) fn new(
        code: impl Into<String>,
        hint: Option<String>,
        retryable: bool,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self {
            code: code.into(),
            hint,
            retryable,
            retry_after_ms,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "hint": self.hint,
            "retryable": self.retryable,
            "retry_after_ms": self.retry_after_ms,
        })
    }
}

pub(crate) fn build_failed_tool_result(
    error: impl Into<String>,
    data: Value,
    meta: ToolErrorMeta,
    sandbox: bool,
) -> Value {
    let data = with_error_meta(data, meta.clone());
    json!({
        "ok": false,
        "data": data,
        "error": error.into(),
        "error_meta": meta.to_json(),
        "sandbox": sandbox,
    })
}

pub(crate) fn with_error_meta(data: Value, meta: ToolErrorMeta) -> Value {
    let mut payload = ensure_object_payload(data);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("error_meta".to_string(), meta.to_json());
    }
    payload
}

pub(crate) fn build_execute_command_failure_data(
    results: &[Value],
    command_count: usize,
    truncated: bool,
    omitted_bytes: usize,
    timed_out: bool,
) -> Value {
    let mut payload = Map::new();
    if let Some(last) = results.last() {
        if let Some(command) = last
            .get("command")
            .cloned()
            .filter(|value| !value.is_null())
        {
            payload.insert("command".to_string(), command);
        }
        if let Some(command_index) = last
            .get("command_index")
            .cloned()
            .filter(|value| !value.is_null())
        {
            payload.insert("command_index".to_string(), command_index);
        }
        if !timed_out {
            if let Some(returncode) = last
                .get("returncode")
                .cloned()
                .filter(|value| !value.is_null())
            {
                payload.insert("returncode".to_string(), returncode);
            }
        }
        if let Some(stderr) = compact_command_output(last.get("stderr").and_then(Value::as_str)) {
            payload.insert("stderr".to_string(), Value::String(stderr));
        } else if let Some(stdout) =
            compact_command_output(last.get("stdout").and_then(Value::as_str))
        {
            payload.insert("stdout".to_string(), Value::String(stdout));
        }
    }
    if timed_out {
        payload.insert("timed_out".to_string(), Value::Bool(true));
    }
    if command_count > 1 {
        payload.insert(
            "command_count".to_string(),
            Value::from(command_count as u64),
        );
        payload.insert(
            "completed_commands".to_string(),
            Value::from(results.len().saturating_sub(1) as u64),
        );
    }
    if truncated {
        payload.insert("output_truncated".to_string(), Value::Bool(true));
        if omitted_bytes > 0 {
            payload.insert(
                "omitted_bytes".to_string(),
                Value::from(omitted_bytes as u64),
            );
        }
    }
    Value::Object(payload)
}

pub(crate) fn build_execute_command_failure_message(results: &[Value], timed_out: bool) -> String {
    if timed_out {
        return "命令执行超时。".to_string();
    }
    if let Some(returncode) = results
        .last()
        .and_then(|item| item.get("returncode"))
        .and_then(Value::as_i64)
    {
        return format!("命令退出码 {returncode}。");
    }
    "命令执行失败。".to_string()
}

fn compact_command_output(raw: Option<&str>) -> Option<String> {
    let text = raw.map(str::trim).filter(|value| !value.is_empty())?;
    let lines = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(3);
    let mut joined = lines[start..].join("\n");
    const MAX_CHARS: usize = 320;
    if joined.chars().count() > MAX_CHARS {
        joined = joined
            .chars()
            .rev()
            .take(MAX_CHARS)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        joined = format!("...{joined}");
    }
    Some(joined)
}

fn ensure_object_payload(data: Value) -> Value {
    if data.is_object() {
        return data;
    }
    let mut map = Map::new();
    map.insert("result".to_string(), data);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_error_meta_wraps_non_object_payload() {
        let payload = with_error_meta(
            Value::String("bad".to_string()),
            ToolErrorMeta::new("TOOL_ERROR", None, false, None),
        );
        let obj = payload.as_object().expect("payload should be object");
        assert_eq!(obj.get("result"), Some(&Value::String("bad".to_string())));
        assert!(obj.get("error_meta").is_some());
    }

    #[test]
    fn build_failed_tool_result_embeds_error_meta_into_data() {
        let payload = build_failed_tool_result(
            "failed",
            json!({ "detail": "bad request" }),
            ToolErrorMeta::new(
                "TOOL_FAILED",
                Some("retry later".to_string()),
                true,
                Some(250),
            ),
            false,
        );
        let data = payload
            .get("data")
            .and_then(Value::as_object)
            .expect("data object");
        let meta = data
            .get("error_meta")
            .and_then(Value::as_object)
            .expect("embedded error_meta");
        assert_eq!(
            meta.get("code"),
            Some(&Value::String("TOOL_FAILED".to_string()))
        );
        assert_eq!(meta.get("retryable"), Some(&Value::Bool(true)));
    }

    #[test]
    fn build_execute_command_failure_data_keeps_only_actionable_tail() {
        let payload = build_execute_command_failure_data(
            &[json!({
                "command": "python draw_heart.py",
                "command_index": 0,
                "returncode": 1,
                "stderr": "Traceback (most recent call last):\n  File \"/tmp/draw_heart.py\", line 6\n    y = 1\nIndentationError: unindent does not match any outer indentation level\n",
                "stdout": ""
            })],
            1,
            false,
            0,
            false,
        );
        assert_eq!(payload["command"], json!("python draw_heart.py"));
        assert_eq!(payload["returncode"], json!(1));
        assert_eq!(
            payload["stderr"],
            json!("  File \"/tmp/draw_heart.py\", line 6\n    y = 1\nIndentationError: unindent does not match any outer indentation level")
        );
        assert!(payload.get("stdout").is_none());
    }
}
