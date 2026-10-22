//! Bounded execution diagnostics attached to stable model/tool items.
use serde_json::{json, Map, Value};
use wunder_core::thread_view::preview;

pub(crate) fn failed(status: &str) -> bool {
    matches!(
        status,
        "failed" | "error" | "cancelled" | "interrupted" | "rejected"
    )
}

fn text(out: &mut Map<String, Value>, key: &str, sources: &[Option<&Value>], cap: usize) {
    if let Some(value) = sources
        .iter()
        .flatten()
        .find_map(|v| v.as_str().filter(|s| !s.trim().is_empty()))
    {
        out.insert(key.into(), json!(preview(value, cap)));
    }
}

/// Keep the last retry only; repeated failures cannot grow the item indefinitely.
pub(crate) fn retry(data: &Value) -> Value {
    let mut out = Map::new();
    for key in ["attempt", "max_attempts", "delay_s"] {
        if let Some(value) = data
            .get(key)
            .filter(|v| v.as_f64().is_some_and(|n| n.is_finite() && n >= 0.0))
        {
            out.insert(key.into(), value.clone());
        }
    }
    text(
        &mut out,
        "reason",
        &[data.get("retry_reason"), data.get("reason")],
        128,
    );
    text(
        &mut out,
        "message",
        &[data.get("error"), data.get("message")],
        2048,
    );
    out.insert(
        "will_retry".into(),
        json!(data
            .get("will_retry")
            .and_then(Value::as_bool)
            .unwrap_or(true)),
    );
    Value::Object(out)
}

/// Do not copy provider request bodies, arbitrary error detail, or tool output.
pub(crate) fn failure(data: &Value, status: &str) -> Option<Value> {
    let error = data
        .get("error")
        .or_else(|| data.get("is_error"))
        .filter(|v| !v.is_null() && **v != false && **v != "");
    if error.is_none()
        && !failed(status)
        && data.get("ok") != Some(&json!(false))
        && data.get("success") != Some(&json!(false))
    {
        return None;
    }
    let mut out = Map::new();
    out.insert(
        "status".into(),
        json!(if failed(status) { status } else { "failed" }),
    );
    text(
        &mut out,
        "code",
        &[
            error.and_then(|v| v.get("code")),
            data.get("code"),
            data.get("stop_reason"),
        ],
        128,
    );
    text(
        &mut out,
        "message",
        &[
            error.and_then(|v| v.get("message")),
            error,
            data.get("message"),
        ],
        2048,
    );
    let meta = error
        .and_then(|v| v.get("error_meta"))
        .or_else(|| data.get("error_meta"));
    text(
        &mut out,
        "source_stage",
        &[
            meta.and_then(|v| v.get("source_stage")),
            data.get("source_stage"),
        ],
        128,
    );
    text(
        &mut out,
        "recovery_action",
        &[
            data.get("recovery_action"),
            meta.and_then(|v| v.get("recovery_action")),
        ],
        128,
    );
    if let Some(value) = data
        .get("retryable")
        .or_else(|| meta.and_then(|v| v.get("retryable")))
        .and_then(Value::as_bool)
    {
        out.insert("retryable".into(), json!(value));
    }
    if let Some(value) = data
        .get("retry_after_ms")
        .or_else(|| meta.and_then(|v| v.get("retry_after_ms")))
        .and_then(Value::as_u64)
    {
        out.insert("retry_after_ms".into(), json!(value));
    }
    Some(Value::Object(out))
}

/// Shared small page projection, also useful for pre-diagnostic terminal logs.
pub(crate) fn project(item: &Value, status: &str) -> Option<Value> {
    let mut out = Map::new();
    if let Some(value) = item.get("trajectory_retry") {
        out.insert("retry".into(), retry(value));
    }
    if let Some(value) = item.get("trajectory_failure") {
        // Re-apply the bound even for imported logs.
        let wrapped = json!({"error": value, "retryable": value.get("retryable"), "retry_after_ms": value.get("retry_after_ms"), "recovery_action": value.get("recovery_action"), "source_stage": value.get("source_stage")});
        if let Some(value) = failure(
            &wrapped,
            value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or(status),
        ) {
            out.insert("failure".into(), value);
        }
    } else if let Some(value) = failure(item, status) {
        out.insert("failure".into(), value);
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_diagnostics_keep_only_bounded_fields() {
        let retry = retry(&json!({"attempt":2,"max_attempts":3,"delay_s":0.1,
            "error":"x".repeat(10000),"detail":{"body":"must not be copied"}}));
        assert_eq!(retry["attempt"], 2);
        assert_eq!(retry["will_retry"], true);
        assert!(retry["message"].as_str().unwrap().len() <= 2048);
        assert!(retry["detail"].is_null());
    }

    #[test]
    fn independent_terminal_diagnostics_preserve_bounded_failure_details() {
        let terminal = json!({"status":"failed","error":{"code":"UPSTREAM_TIMEOUT",
            "message":"x".repeat(10000),"detail":{"large":"do not copy"},
            "error_meta":{"source_stage":"llm","retryable":true,"recovery_action":"retry"}}});
        let projection = project(&terminal, "failed").unwrap();
        let failure = &projection["failure"];
        assert_eq!(failure["code"], "UPSTREAM_TIMEOUT");
        assert_eq!(failure["source_stage"], "llm");
        assert_eq!(failure["retryable"], true);
        assert_eq!(failure["recovery_action"], "retry");
        assert!(failure["message"].as_str().unwrap().len() <= 2048);
        assert!(failure["detail"].is_null());
        assert!(project(&json!({"content":"completed response"}), "completed").is_none());
    }
}
