use super::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MAX_SAME_NON_RETRYABLE_FAILURES: u32 = 5;
// Deterministic argument/schema failures cannot be fixed by waiting. Two
// observations are enough for one reroute hint, without changing the budget
// for executable failures such as syntax or permission errors.
const MAX_SAME_ARGUMENT_FAILURES: u32 = 2;
const MAX_SAME_RETRYABLE_FAILURES: u32 = 5;
const MAX_SAME_TOOL_FAILURES: u32 = 5;
const MAX_SAME_APPLY_PATCH_FAILURES: u32 = 5;
const MAX_SAME_SUCCESS_NO_PROGRESS: u32 = 4;
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

/// Detect a successful tool loop separately from failure retries. A command
/// can return `ok=true` forever while producing the same observation; failure
/// guards cannot see that case. Keep the detector narrow to command-like tools
/// and require several identical non-empty observations to avoid stopping
/// ordinary polling or idempotent writes.
#[derive(Clone, Debug, Default)]
pub(super) struct SuccessProgressGovernor {
    last_tool: String,
    last_fingerprint: String,
    repeat_count: u32,
}

#[derive(Clone, Debug)]
pub(super) struct SuccessProgressStop {
    pub(super) tool: String,
    pub(super) repeat_count: u32,
    pub(super) threshold: u32,
    pub(super) detail: String,
}

impl SuccessProgressGovernor {
    pub(super) fn reset(&mut self) {
        self.last_tool.clear();
        self.last_fingerprint.clear();
        self.repeat_count = 0;
    }

    pub(super) fn record(
        &mut self,
        tool_name: &str,
        args: &Value,
        result: &ToolResultPayload,
    ) -> Option<SuccessProgressStop> {
        if !result.ok {
            self.reset();
            return None;
        }
        let Some((fingerprint, detail)) = success_progress_fingerprint(tool_name, args, result)
        else {
            self.reset();
            return None;
        };
        if self.last_tool == tool_name && self.last_fingerprint == fingerprint {
            self.repeat_count = self.repeat_count.saturating_add(1);
        } else {
            self.last_tool = tool_name.to_string();
            self.last_fingerprint = fingerprint;
            self.repeat_count = 1;
        }
        (self.repeat_count >= MAX_SAME_SUCCESS_NO_PROGRESS).then(|| SuccessProgressStop {
            tool: tool_name.to_string(),
            repeat_count: self.repeat_count,
            threshold: MAX_SAME_SUCCESS_NO_PROGRESS,
            detail,
        })
    }
}

fn success_progress_fingerprint(
    tool_name: &str,
    args: &Value,
    result: &ToolResultPayload,
) -> Option<(String, String)> {
    let canonical = resolve_tool_name(tool_name);
    if !matches!(
        canonical.as_str(),
        "执行命令" | "execute_command" | "ptc" | "程序化工具调用"
    ) {
        return None;
    }
    let mut fingerprint_fragments = Vec::new();
    let mut observation_fragments = Vec::new();
    // Include the requested command/script in the fingerprint. Different
    // probes often share an empty or short stdout; comparing output alone
    // would stop legitimate progress after four unrelated calls.
    if let Some(command) = args.as_object().and_then(|object| {
        ["content", "command", "cmd"]
            .iter()
            .find_map(|key| object.get(*key).and_then(Value::as_str))
    }) {
        let command = command.split_whitespace().collect::<Vec<_>>().join(" ");
        if !command.is_empty() {
            let mut hasher = DefaultHasher::new();
            command.hash(&mut hasher);
            fingerprint_fragments.push(format!("command_hash:{:016x}", hasher.finish()));
        }
    }
    if let Some(stdout) = result.data.get("stdout").and_then(Value::as_str) {
        if !stdout.trim().is_empty() {
            let fragment = format!("stdout:{}", compact_progress_text(stdout));
            fingerprint_fragments.push(fragment.clone());
            observation_fragments.push(fragment);
        }
    }
    if let Some(stderr) = result.data.get("stderr").and_then(Value::as_str) {
        if !stderr.trim().is_empty() {
            let fragment = format!("stderr:{}", compact_progress_text(stderr));
            fingerprint_fragments.push(fragment.clone());
            observation_fragments.push(fragment);
        }
    }
    if let Some(rows) = result.data.get("results").and_then(Value::as_array) {
        for row in rows {
            let Some(row) = row.as_object() else { continue };
            if let Some(code) = row.get("returncode") {
                let fragment = format!("returncode:{code}");
                fingerprint_fragments.push(fragment.clone());
                observation_fragments.push(fragment);
            }
            for key in ["stdout", "stderr"] {
                if let Some(text) = row.get(key).and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        let fragment = format!("{key}:{}", compact_progress_text(text));
                        fingerprint_fragments.push(fragment.clone());
                        observation_fragments.push(fragment);
                    }
                }
            }
        }
    }
    if observation_fragments.is_empty() {
        return None;
    }
    let detail = observation_fragments.join(" | ");
    let fingerprint = normalize_detail(&fingerprint_fragments.join(" | "));
    Some((
        fingerprint,
        detail.chars().take(FINGERPRINT_DETAIL_MAX_CHARS).collect(),
    ))
}

fn compact_progress_text(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(FINGERPRINT_DETAIL_MAX_CHARS)
        .collect()
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
        let apply_patch = resolve_tool_name("apply_patch");
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

        let non_retryable_threshold = if is_argument_failure_code(&fingerprint.code) {
            MAX_SAME_ARGUMENT_FAILURES
        } else {
            MAX_SAME_NON_RETRYABLE_FAILURES
        };
        if !fingerprint.retryable && self.same_fingerprint_failures >= non_retryable_threshold {
            return Some(RetryStopDecision {
                reason: "same_non_retryable_failure",
                fingerprint: fingerprint.key,
                repeat_count: self.same_fingerprint_failures,
                same_tool_failures: self.same_tool_failures,
                threshold: non_retryable_threshold,
                retryable: false,
                error_code: fingerprint.code,
                detail: fingerprint.detail,
            });
        }

        if tool_name != apply_patch
            && fingerprint.retryable
            && self.same_fingerprint_failures >= MAX_SAME_RETRYABLE_FAILURES
        {
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

        if tool_name == apply_patch && self.same_tool_failures >= MAX_SAME_APPLY_PATCH_FAILURES {
            return Some(RetryStopDecision {
                reason: "tool_failure_reroute_required",
                fingerprint: fingerprint.key,
                repeat_count: self.same_fingerprint_failures,
                same_tool_failures: self.same_tool_failures,
                threshold: MAX_SAME_APPLY_PATCH_FAILURES,
                retryable: fingerprint.retryable,
                error_code: fingerprint.code,
                detail: fingerprint.detail,
            });
        }

        if self.same_tool_failures >= MAX_SAME_TOOL_FAILURES && self.same_fingerprint_failures == 1
        {
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

fn is_argument_failure_code(code: &str) -> bool {
    let code = code.trim().to_ascii_uppercase();
    code.contains("ARGS")
        || code.contains("ARGUMENT")
        || code.ends_with("_REQUIRED")
        || code.ends_with("_MISSING_FIELD")
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
    use super::{
        RetryGovernor, SuccessProgressGovernor, ToolResultPayload, MAX_SAME_ARGUMENT_FAILURES,
        MAX_SAME_NON_RETRYABLE_FAILURES, MAX_SAME_RETRYABLE_FAILURES,
    };
    use crate::tools::resolve_tool_name;
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
        for _ in 0..(MAX_SAME_NON_RETRYABLE_FAILURES - 1) {
            assert!(governor.record_failure("ptc", &payload).is_none());
        }
        let stop = governor
            .record_failure("ptc", &payload)
            .expect("should stop when same non-retryable failures reach the default threshold");
        assert_eq!(stop.reason, "same_non_retryable_failure");
        assert_eq!(stop.threshold, MAX_SAME_NON_RETRYABLE_FAILURES);
    }

    #[test]
    fn stops_on_repeated_argument_failure_after_two_observations() {
        let mut governor = RetryGovernor::new(5);
        let payload = ToolResultPayload {
            ok: false,
            data: json!({}),
            error: "missing path".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: Some(json!({
                "error_code": "TOOL_WRITE_PATH_REQUIRED",
                "error_retryable": false
            })),
        };
        assert!(governor.record_failure("write_file", &payload).is_none());
        let stop = governor
            .record_failure("write_file", &payload)
            .expect("argument failures should reroute quickly");
        assert_eq!(stop.threshold, MAX_SAME_ARGUMENT_FAILURES);
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
        for _ in 0..(MAX_SAME_RETRYABLE_FAILURES - 1) {
            assert!(governor.record_failure("read_file", &payload).is_none());
        }
        let stop = governor
            .record_failure("read_file", &payload)
            .expect("should stop when same retryable failures reach the default threshold");
        assert_eq!(stop.reason, "same_retryable_failure_exhausted");
    }

    #[test]
    fn apply_patch_reroutes_after_five_failures() {
        let mut governor = RetryGovernor::new(6);
        let tool_name = resolve_tool_name("apply_patch");
        let payload = ToolResultPayload {
            ok: false,
            data: json!({
                "error_meta": {
                    "code": "PATCH_CONTEXT_NOT_FOUND",
                    "retryable": true
                }
            }),
            error: "Patch apply failed: chunk 2 in demo.txt has no matching context".to_string(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        assert!(governor.record_failure(&tool_name, &payload).is_none());
        assert!(governor.record_failure(&tool_name, &payload).is_none());
        assert!(governor.record_failure(&tool_name, &payload).is_none());
        assert!(governor.record_failure(&tool_name, &payload).is_none());
        let stop = governor
            .record_failure(&tool_name, &payload)
            .expect("apply_patch should reroute on fifth failure");
        assert_eq!(stop.reason, "tool_failure_reroute_required");
        assert_eq!(stop.threshold, 5);
        assert_eq!(stop.same_tool_failures, 5);
    }

    #[test]
    fn stops_on_repeated_successful_command_observation() {
        let mut governor = SuccessProgressGovernor::default();
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "results": [{"returncode": 0, "stdout": "n_frames: 1 is_animated: False\n"}]
            }),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        for _ in 0..3 {
            assert!(governor
                .record(
                    "execute_command",
                    &json!({"command": "inspect-gif"}),
                    &payload
                )
                .is_none());
        }
        let stop = governor
            .record(
                "execute_command",
                &json!({"command": "inspect-gif"}),
                &payload,
            )
            .expect("identical successful command observations should stop the loop");
        assert_eq!(stop.repeat_count, 4);
        assert!(stop.detail.contains("n_frames: 1"));
    }

    #[test]
    fn different_successful_commands_reset_no_progress_counter() {
        let mut governor = SuccessProgressGovernor::default();
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "results": [{"returncode": 0, "stdout": "same probe result"}]
            }),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        for index in 0..8 {
            let args = json!({"command": format!("probe-{index}")});
            assert!(governor
                .record("execute_command", &args, &payload)
                .is_none());
        }
    }
}
