use super::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MAX_SAME_RETRYABLE_FAILURES: u32 = 2;
const MAX_SAME_TOOL_FAILURES: u32 = 3;
const FINGERPRINT_DETAIL_MAX_CHARS: usize = 240;

#[derive(Clone, Debug)]
pub(super) struct RetryStopDecision {
    pub(super) reason: &'static str,
    pub(super) fingerprint: String,
    pub(super) repeat_count: u32,
    pub(super) same_tool_failures: u32,
    pub(super) threshold: u32,
    pub(super) retryable: bool,
    pub(super) error_code: String,
    pub(super) detail: String,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RetryGovernor {
    hard_threshold: u32,
    last_fingerprint: String,
    same_fingerprint_failures: u32,
    last_tool: String,
    same_tool_failures: u32,
}

#[derive(Clone, Debug)]
struct ToolFailureFingerprint {
    key: String,
    code: String,
    detail: String,
    retryable: bool,
}

impl RetryGovernor {
    pub(super) fn new(hard_threshold: u32) -> Self {
        Self {
            hard_threshold: hard_threshold.max(1),
            ..Self::default()
        }
    }

    pub(super) fn record_success(&mut self) {
        self.last_fingerprint.clear();
        self.same_fingerprint_failures = 0;
        self.last_tool.clear();
        self.same_tool_failures = 0;
    }

    pub(super) fn record_failure(
        &mut self,
        tool_name: &str,
        result: &ToolResultPayload,
    ) -> Option<RetryStopDecision> {
        let fingerprint = ToolFailureFingerprint::from_result(tool_name, result);
        if fingerprint.key == self.last_fingerprint {
            self.same_fingerprint_failures = self.same_fingerprint_failures.saturating_add(1);
        } else {
            self.last_fingerprint = fingerprint.key.clone();
            self.same_fingerprint_failures = 1;
        }

        if tool_name == self.last_tool {
            self.same_tool_failures = self.same_tool_failures.saturating_add(1);
        } else {
            self.last_tool = tool_name.to_string();
            self.same_tool_failures = 1;
        }

        if !fingerprint.retryable && self.same_fingerprint_failures >= 2 {
            return Some(RetryStopDecision {
                reason: "same_non_retryable_failure",
                fingerprint: fingerprint.key,
                repeat_count: self.same_fingerprint_failures,
                same_tool_failures: self.same_tool_failures,
                threshold: 2,
                retryable: false,
                error_code: fingerprint.code,
                detail: fingerprint.detail,
            });
        }

        if fingerprint.retryable && self.same_fingerprint_failures > MAX_SAME_RETRYABLE_FAILURES {
            return Some(RetryStopDecision {
                reason: "same_retryable_failure_exhausted",
                fingerprint: fingerprint.key,
                repeat_count: self.same_fingerprint_failures,
                same_tool_failures: self.same_tool_failures,
                threshold: MAX_SAME_RETRYABLE_FAILURES,
                retryable: true,
                error_code: fingerprint.code,
                detail: fingerprint.detail,
            });
        }

        if self.same_tool_failures >= MAX_SAME_TOOL_FAILURES && self.same_fingerprint_failures == 1 {
            return Some(RetryStopDecision {
                reason: "tool_failure_reroute_required",
                fingerprint: fingerprint.key,
                repeat_count: self.same_fingerprint_failures,
                same_tool_failures: self.same_tool_failures,
                threshold: MAX_SAME_TOOL_FAILURES,
                retryable: fingerprint.retryable,
                error_code: fingerprint.code,
                detail: fingerprint.detail,
            });
        }

        if self.same_fingerprint_failures >= self.hard_threshold {
            return Some(RetryStopDecision {
                reason: "failure_guard_threshold_reached",
                fingerprint: fingerprint.key,
                repeat_count: self.same_fingerprint_failures,
                same_tool_failures: self.same_tool_failures,
                threshold: self.hard_threshold,
                retryable: fingerprint.retryable,
                error_code: fingerprint.code,
                detail: fingerprint.detail,
            });
        }

        None
    }
}

impl ToolFailureFingerprint {
    fn from_result(tool_name: &str, result: &ToolResultPayload) -> Self {
        let (code, retryable) = extract_error_code_and_retryable(result);
        let detail = extract_failure_detail(result);
        let preflight_code = extract_preflight_code(result);
        let normalized_detail = normalize_detail(&detail);
        let raw_key = format!(
            "{tool}|{code}|{preflight}|{detail}",
            tool = tool_name.trim(),
            code = code,
            preflight = preflight_code.unwrap_or_default(),
            detail = normalized_detail,
        );
        let mut hasher = DefaultHasher::new();
        raw_key.hash(&mut hasher);
        let digest = format!("{:016x}", hasher.finish());
        let key = format!("{code}:{digest}");
        Self {
            key,
            code,
            detail: normalized_detail,
            retryable,
        }
    }
}

fn extract_error_code_and_retryable(result: &ToolResultPayload) -> (String, bool) {
    if let Some(meta) = result.meta.as_ref().and_then(Value::as_object) {
        let code = meta
            .get("error_code")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let retryable = meta.get("error_retryable").and_then(Value::as_bool);
        if let (Some(code), Some(retryable)) = (code, retryable) {
            return (code, retryable);
        }
    }

    let code = result
        .data
        .get("error_meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "TOOL_EXEC_FAILED".to_string());
    let retryable = result
        .data
        .get("error_meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("retryable"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (code, retryable)
}

fn extract_failure_detail(result: &ToolResultPayload) -> String {
    if !result.error.trim().is_empty() {
        return result.error.trim().to_string();
    }
    if let Some(text) = result
        .meta
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("error_detail_head"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return text.to_string();
    }
    serde_json::to_string(&result.data).unwrap_or_else(|_| "{}".to_string())
}

fn extract_preflight_code(result: &ToolResultPayload) -> Option<String> {
    result
        .meta
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("preflight"))
        .and_then(Value::as_object)
        .and_then(|preflight| preflight.get("code"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn normalize_detail(detail: &str) -> String {
    let collapsed = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .chars()
        .take(FINGERPRINT_DETAIL_MAX_CHARS)
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::{RetryGovernor, ToolResultPayload};
    use chrono::Utc;
    use serde_json::json;

    #[test]
    fn stops_on_repeated_non_retryable_fingerprint() {
        let mut governor = RetryGovernor::new(5);
        let payload = ToolResultPayload {
            ok: false,
            data: json!({}),
            error: "SyntaxError: invalid syntax".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "error_code": "PYTHON_SYNTAX_ERROR",
                "error_retryable": false
            })),
        };
        assert!(governor.record_failure("ptc", &payload).is_none());
        let stop = governor
            .record_failure("ptc", &payload)
            .expect("should stop on second same non-retryable failure");
        assert_eq!(stop.reason, "same_non_retryable_failure");
        assert_eq!(stop.threshold, 2);
    }

    #[test]
    fn stops_on_retryable_fingerprint_after_budget() {
        let mut governor = RetryGovernor::new(6);
        let payload = ToolResultPayload {
            ok: false,
            data: json!({}),
            error: "timeout while calling service".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "error_code": "TOOL_TIMEOUT",
                "error_retryable": true
            })),
        };
        assert!(governor.record_failure("read_file", &payload).is_none());
        assert!(governor.record_failure("read_file", &payload).is_none());
        let stop = governor
            .record_failure("read_file", &payload)
            .expect("should stop on third same retryable failure");
        assert_eq!(stop.reason, "same_retryable_failure_exhausted");
    }
}
