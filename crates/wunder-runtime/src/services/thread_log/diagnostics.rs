//! Bounded execution diagnostics attached to stable model/tool items.
use serde_json::{json, Map, Value};
use std::borrow::Cow;
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

/// A patch updates the admitted assistant identity, never appends a noise row.
pub(crate) fn event_patch(session: &str, event: &str, data: &Value) -> Option<Value> {
    if !matches!(event, "llm_stream_retry" | "turn_terminal") {
        return None;
    }
    let turn = data.get("turn_id")?.as_str()?;
    let model = data.get("model_round")?.as_i64().filter(|v| *v > 0)?;
    let mut patch = json!({
        "session_id": session, "turn_id": turn, "item_id": format!("{turn}:text-{model}"),
        "kind": "assistant_message", "role": "assistant", "visibility": "user",
        "model_round": model, "user_round": data.get("user_round"), "_trajectory_patch": true,
    });
    match event {
        "llm_stream_retry" => {
            patch["status"] = json!("running");
            patch["trajectory_retry"] = retry(data);
        }
        "turn_terminal" => {
            if data.get("error").is_none()
                && !failed(data.get("status").and_then(Value::as_str).unwrap_or(""))
            {
                return None;
            }
            let status = data
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("failed");
            patch["trajectory_failure"] = failure(data, status)?;
            patch["status"] = json!(status);
        }
        _ => return None,
    }
    Some(patch)
}

/// Called inside each store's existing item transaction. Full response writes
/// retain diagnostics; diagnostic patches retain the response and reasoning.
pub(crate) fn merge_item<'a>(incoming: &'a Value, previous: Option<&str>) -> Cow<'a, Value> {
    let patch = incoming.get("_trajectory_patch") == Some(&json!(true));
    let has_diagnostics = previous.is_some_and(|s| {
        s.contains("\"trajectory_retry\"") || s.contains("\"trajectory_failure\"")
    });
    if !patch && !has_diagnostics {
        return Cow::Borrowed(incoming);
    }
    let previous = previous
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or_else(|| json!({}));
    let mut merged = if patch {
        previous
    } else {
        let mut merged = incoming.clone();
        for key in ["trajectory_retry", "trajectory_failure"] {
            if merged.get(key).is_none() {
                if let Some(value) = previous.get(key) {
                    merged[key] = value.clone();
                }
            }
        }
        merged
    };
    if patch {
        if let (Some(target), Some(source)) = (merged.as_object_mut(), incoming.as_object()) {
            for (key, value) in source {
                if key != "_trajectory_patch" {
                    target.insert(key.clone(), value.clone());
                }
            }
        }
    } else {
        // Late history/usage writes must not turn a terminal failure green.
        if let Some(status) = merged
            .pointer("/trajectory_failure/status")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            merged["status"] = json!(status);
        }
    }
    if merged.get("status").and_then(Value::as_str) != Some("running")
        && merged.get("trajectory_retry").is_some()
    {
        merged["trajectory_retry"]["will_retry"] = json!(false);
    }
    Cow::Owned(merged)
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
    fn retry_then_success_and_late_history_preserve_last_attempt() {
        let initial = json!({"content":"partial", "reasoning":"thought", "status":"running"});
        let retry_patch = event_patch("session", "llm_stream_retry", &json!({"turn_id":"turn", "model_round":1, "attempt":2,"max_attempts":3,"error":"temporary failure"})).unwrap();
        let merged = merge_item(&retry_patch, Some(&initial.to_string())).into_owned();
        assert_eq!(merged["content"], "partial");
        assert_eq!(merged["reasoning"], "thought");
        assert!(merged.get("_trajectory_patch").is_none());
        let success = merge_item(
            &json!({"content":"done","status":"completed"}),
            Some(&merged.to_string()),
        )
        .into_owned();
        assert_eq!(success["trajectory_retry"]["will_retry"], false);
        assert_eq!(success["trajectory_retry"]["attempt"], 2);
        assert_eq!(
            project(&success, "completed").unwrap()["retry"]["message"],
            "temporary failure"
        );
    }

    #[test]
    fn failure_is_bounded_and_survives_late_success_payloads() {
        let patch = event_patch("session", "turn_terminal", &json!({"turn_id":"turn","model_round":1,"status":"failed","error":{"code":"UPSTREAM_TIMEOUT","message":"x".repeat(10000),"detail":{"large":"do not copy"}},"retryable":true,"recovery_action":"retry"})).unwrap();
        assert!(
            patch["trajectory_failure"]["message"]
                .as_str()
                .unwrap()
                .len()
                <= 2048
        );
        assert!(patch["trajectory_failure"].get("detail").is_none());
        let settled = merge_item(
            &patch,
            Some(r#"{"content":"partial","reasoning":"thought"}"#),
        )
        .into_owned();
        let late = merge_item(
            &json!({"content":"partial","status":"completed"}),
            Some(&settled.to_string()),
        )
        .into_owned();
        assert_eq!(late["status"], "failed");
        assert_eq!(late["trajectory_failure"]["code"], "UPSTREAM_TIMEOUT");
        assert!(event_patch(
            "session",
            "turn_terminal",
            &json!({"turn_id":"turn","status":"failed"})
        )
        .is_none());
    }
}
