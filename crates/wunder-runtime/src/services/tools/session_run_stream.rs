use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use anyhow::{anyhow, Result};
use futures::StreamExt;
use serde_json::Value;
use std::sync::Arc;

pub(super) struct SessionRunStreamOutcome {
    pub answer: Option<String>,
}

pub(super) fn run_request(
    orchestrator: Arc<Orchestrator>,
    request: WunderRequest,
    cancellation: Option<tokio_util::sync::CancellationToken>,
) -> futures::future::BoxFuture<'static, Result<SessionRunStreamOutcome>> {
    // Box the recursive tool -> child -> orchestrator future at this boundary.
    Box::pin(async move {
        if cancellation
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
        {
            return Err(anyhow!("interrupted"));
        }
        let mut stream = Box::pin(orchestrator.stream(request).await?);
        let mut outcome = None;
        let mut failure = None;
        while let Some(event) = stream.next().await {
            let event = match event {
                Ok(item) => item,
                Err(_) => continue,
            };
            let payload = event
                .data
                .get("data")
                .cloned()
                .unwrap_or_else(|| event.data.clone());
            let event_name = event.event.trim().to_ascii_lowercase();
            if event_name == "error" {
                failure = Some(extract_error_text(&payload, &event.data));
                continue;
            }
            if event_name != "final" {
                continue;
            }
            let answer = payload
                .get("answer")
                .or_else(|| payload.get("content"))
                .or_else(|| payload.get("message"))
                .or_else(|| event.data.get("answer"))
                .or_else(|| event.data.get("content"))
                .or_else(|| event.data.get("message"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            outcome = Some(SessionRunStreamOutcome { answer });
        }
        // Drain settlement before releasing ownership of the reusable child session.
        if let Some(failure) = failure {
            return Err(anyhow!(failure));
        }
        outcome.ok_or_else(|| anyhow!("child stream ended without a final response"))
    })
}

fn extract_error_text(payload: &Value, fallback: &Value) -> String {
    let code = payload
        .get("code")
        .and_then(Value::as_str)
        .or_else(|| fallback.get("code").and_then(Value::as_str))
        .map(str::trim)
        .unwrap_or("");
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| fallback.get("message").and_then(Value::as_str))
        .map(str::trim)
        .unwrap_or("");
    let detail = if message.is_empty() {
        serde_json::to_string(payload).unwrap_or_default()
    } else {
        message.to_string()
    };
    if code.is_empty() {
        detail
    } else {
        format!("code={code}, message={detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::extract_error_text;
    use serde_json::json;

    #[test]
    fn extract_error_text_prefers_payload_code_and_message() {
        let payload = json!({
            "code": "USER_BUSY",
            "message": "busy now",
        });
        let fallback = json!({
            "code": "INTERNAL_ERROR",
            "message": "fallback",
        });
        assert_eq!(
            extract_error_text(&payload, &fallback),
            "code=USER_BUSY, message=busy now"
        );
    }

    #[test]
    fn extract_error_text_falls_back_to_serialized_payload() {
        let payload = json!({
            "detail": { "trace": "abc" }
        });
        let fallback = json!({});
        let text = extract_error_text(&payload, &fallback);
        assert!(text.contains("\"detail\""));
        assert!(text.contains("\"trace\""));
    }
}
