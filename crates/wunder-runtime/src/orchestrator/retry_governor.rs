use super::*;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::collections::VecDeque;
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
// A stalled agent rarely repeats one byte-identical call: it usually alternates
// between two "patch" and "verify" calls whose arguments differ slightly while
// the observations keep repeating. Watch a short window of successful calls and
// treat a window filled by at most two recurring observations as no progress.
const SUCCESS_STALL_WINDOW: usize = 8;
const SUCCESS_STALL_DISTINCT_LIMIT: usize = 2;
// Warnings a single turn may spend on freshly-typed stalled patterns before the
// guard stops trusting the model to escape on its own.
const MAX_SUCCESS_NO_PROGRESS_WARNINGS: u32 = 3;
// A real loop often mixes a few slightly different probes (write, grep, sed,
// od) so no single call repeats and the distinct-observation count stays above
// two. What never changes is the *content*: every new call only prints lines
// that the window already saw. This streak threshold catches that shape without
// needing identical calls.
const SUCCESS_STALL_NO_NEW_LINE_STREAK: usize = 6;
// Successful command steps kept as the "already printed" baseline. Long enough
// to cover a slow patch/verify cycle, short enough that an old probe cannot make
// fresh work look recycled.
const SUCCESS_STALL_HISTORY: usize = 24;
const OBSERVATION_MAX_LINES: usize = 200;
const OBSERVATION_LINE_MAX_CHARS: usize = 200;
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
/// guards cannot see that case.
///
/// Command-like calls are judged on their observations. Write-family calls are
/// judged on path, requested content hash and the real diff counts, so a
/// rewrite that leaves the file byte-identical is visible as no progress even
/// though the tool answered `ok=true` (a real incident repeated one no-op
/// write 86 times and burned a whole turn's budget).
///
/// Three shapes are covered:
/// * the same call repeated verbatim (`MAX_SAME_SUCCESS_NO_PROGRESS`),
/// * a short cycle where one or two calls alternate while their observations
///   never change (`SUCCESS_STALL_WINDOW`), and
/// * a call sequence that keeps changing its command tail but only ever reprints
///   lines the turn has already seen (`SUCCESS_STALL_NO_NEW_LINE_STREAK`). This
///   is what a model does when it re-runs "patch, then look" without the patch
///   ever taking effect.
///
/// The first detection only warns the model so it can change strategy; a relapse
/// of the same evidence, or repeated fresh-but-stalled patterns, escalates to a
/// hard stop so the loop cannot run until the round budget is exhausted.
#[derive(Clone, Debug, Default)]
pub(super) struct SuccessProgressGovernor {
    last_tool: String,
    last_fingerprint: String,
    repeat_count: u32,
    /// Recent successful command steps. The tail (`SUCCESS_STALL_WINDOW`) drives
    /// the cycle detector; the whole queue is the baseline for "already seen".
    recent: VecDeque<SuccessStep>,
    warnings: u32,
    previous_trigger_signatures: Vec<String>,
}

#[derive(Clone, Debug)]
struct SuccessStep {
    signature: String,
    detail: String,
    /// Normalized first command segment. A loop that keeps re-running the same
    /// script with different verification tails shares this prefix while
    /// unrelated probes (different patterns, different files) do not.
    subject: String,
    /// Normalized observation lines; used to prove that a call printed nothing
    /// the turn had not already printed.
    lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub(super) struct SuccessProgressStop {
    pub(super) tool: String,
    pub(super) repeat_count: u32,
    pub(super) threshold: u32,
    pub(super) detail: String,
    /// Distinct observations the stalled window kept replaying (`0` when the
    /// trigger came from recycled output instead of repeated calls).
    pub(super) distinct_steps: usize,
    /// `false` on the first detection (warn only), `true` when the model already
    /// ignored a warning and the turn must stop.
    pub(super) escalated: bool,
}

impl SuccessProgressGovernor {
    pub(super) fn reset(&mut self) {
        self.last_tool.clear();
        self.last_fingerprint.clear();
        self.repeat_count = 0;
        self.recent.clear();
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
        let signature = format!("{tool_name}|{fingerprint}");
        if self.last_tool == tool_name && self.last_fingerprint == signature {
            self.repeat_count = self.repeat_count.saturating_add(1);
        } else {
            self.last_tool = tool_name.to_string();
            self.last_fingerprint = signature.clone();
            self.repeat_count = 1;
        }
        self.recent.push_back(SuccessStep {
            signature,
            detail: detail.clone(),
            subject: success_command_subject(args),
            lines: success_observation_lines(result),
        });
        while self.recent.len() > SUCCESS_STALL_HISTORY {
            self.recent.pop_front();
        }

        let (repeat_count, distinct_steps, threshold) =
            if self.repeat_count >= MAX_SAME_SUCCESS_NO_PROGRESS {
                (self.repeat_count, 1usize, MAX_SAME_SUCCESS_NO_PROGRESS)
            } else if let Some(distinct_steps) = stalled_window_distinct(&self.recent) {
                (
                    u32::try_from(SUCCESS_STALL_WINDOW).unwrap_or(u32::MAX),
                    distinct_steps,
                    u32::try_from(SUCCESS_STALL_WINDOW).unwrap_or(u32::MAX),
                )
            } else if let Some(streak) = stalled_without_new_lines(&self.recent) {
                (
                    u32::try_from(streak).unwrap_or(u32::MAX),
                    0usize,
                    u32::try_from(SUCCESS_STALL_NO_NEW_LINE_STREAK).unwrap_or(u32::MAX),
                )
            } else {
                return None;
            };

        // Only a relapse of the same evidence escalates. A different stalled
        // pattern means the model did try something else, so it earns a warning
        // again instead of being cut off for a loop it already escaped.
        let trigger_signatures: Vec<String> = self
            .recent
            .iter()
            .skip(self.recent.len().saturating_sub(SUCCESS_STALL_WINDOW))
            .map(|step| step.signature.clone())
            .collect();
        let repeated_evidence = !self.previous_trigger_signatures.is_empty()
            && !trigger_signatures.is_empty()
            && trigger_signatures
                .iter()
                .all(|signature| self.previous_trigger_signatures.contains(signature));
        self.warnings = self.warnings.saturating_add(1);
        let escalated = repeated_evidence || self.warnings > MAX_SUCCESS_NO_PROGRESS_WARNINGS;
        self.previous_trigger_signatures = trigger_signatures;
        // Drop the evidence so the model gets a full window to prove it changed
        // strategy; a relapse has to rebuild the same pattern before escalating.
        self.recent.clear();
        self.repeat_count = 0;
        self.last_fingerprint.clear();
        Some(SuccessProgressStop {
            tool: tool_name.to_string(),
            repeat_count,
            threshold,
            detail,
            distinct_steps,
            escalated,
        })
    }
}

/// Normalized observation lines for a successful tool result. Only command
/// stdout/stderr feeds the "no new information" check; the exact same text is
/// already part of the fingerprint, so this stays a cheap per-call scan.
fn success_observation_lines(result: &ToolResultPayload) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut collect = |text: &str| {
        for line in text.lines() {
            if lines.len() >= OBSERVATION_MAX_LINES {
                return;
            }
            let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if normalized.is_empty() {
                continue;
            }
            lines.push(
                normalized
                    .chars()
                    .take(OBSERVATION_LINE_MAX_CHARS)
                    .collect(),
            );
        }
    };
    if let Some(text) = result.data.get("stdout").and_then(Value::as_str) {
        collect(text);
    }
    if let Some(text) = result.data.get("stderr").and_then(Value::as_str) {
        collect(text);
    }
    if let Some(rows) = result.data.get("results").and_then(Value::as_array) {
        for row in rows {
            let Some(row) = row.as_object() else { continue };
            for key in ["stdout", "stderr"] {
                if let Some(text) = row.get(key).and_then(Value::as_str) {
                    collect(text);
                }
            }
        }
    }
    lines
}

/// Streak of trailing calls whose observation printed only lines the window had
/// already printed. Catches loops that vary their commands (so no call repeats)
/// while every result is recycled content, which is what a model does when it
/// keeps re-reading a file it believes it just changed.
fn stalled_without_new_lines(recent: &VecDeque<SuccessStep>) -> Option<usize> {
    if recent.len() < SUCCESS_STALL_NO_NEW_LINE_STREAK {
        return None;
    }
    // Everything older than the trailing window is the "already seen" baseline:
    // reprinting it is no new information for the model.
    let baseline_len = recent.len().saturating_sub(SUCCESS_STALL_WINDOW);
    let mut seen: HashSet<&str> = HashSet::new();
    for step in recent.iter().take(baseline_len) {
        for line in &step.lines {
            seen.insert(line.as_str());
        }
    }

    let mut streak = 0usize;
    let mut streak_subjects: Vec<&str> = Vec::new();
    for step in recent.iter().skip(baseline_len) {
        if step.lines.is_empty() {
            // An empty observation proves nothing; never count it either way.
            streak = 0;
            streak_subjects.clear();
            continue;
        }
        let introduces_new_line = step.lines.iter().any(|line| !seen.contains(line.as_str()));
        for line in &step.lines {
            seen.insert(line.as_str());
        }
        if introduces_new_line {
            streak = 0;
            streak_subjects.clear();
        } else {
            streak += 1;
            streak_subjects.push(step.subject.as_str());
        }
    }
    if streak < SUCCESS_STALL_NO_NEW_LINE_STREAK {
        return None;
    }
    // Recycled output alone is not a loop: six searches over different targets
    // can legitimately print the same shared lines. Require the streak to keep
    // re-running the same command subject, which is what "still no progress"
    // looks like in practice.
    let mut subjects: Vec<&str> = Vec::new();
    for subject in &streak_subjects {
        if !subjects.iter().any(|seen| *seen == *subject) {
            subjects.push(subject);
            if subjects.len() > SUCCESS_STALL_DISTINCT_LIMIT {
                return None;
            }
        }
    }
    Some(streak)
}

/// Command text up to the first `;`, normalized. Verification tails (`; grep`,
/// `; sed -n`) drop out, so re-running the same script keeps one subject while
/// unrelated probes keep distinct ones.
fn success_command_subject(args: &Value) -> String {
    let Some(command) = args.as_object().and_then(|object| {
        ["content", "command", "cmd"]
            .iter()
            .find_map(|key| object.get(*key).and_then(Value::as_str))
    }) else {
        return String::new();
    };
    let head = command.split(';').next().unwrap_or(command);
    head.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(FINGERPRINT_DETAIL_MAX_CHARS)
        .collect()
}

/// Returns how many distinct observations a full window kept replaying when the
/// window looks like a stalled cycle. Every observation must recur, otherwise a
/// single fresh probe followed by replayed calls would be misread as a loop.
fn stalled_window_distinct(recent: &VecDeque<SuccessStep>) -> Option<usize> {
    if recent.len() < SUCCESS_STALL_WINDOW {
        return None;
    }
    let window: Vec<&SuccessStep> = recent
        .iter()
        .skip(recent.len().saturating_sub(SUCCESS_STALL_WINDOW))
        .collect();
    let mut distinct: Vec<&str> = Vec::new();
    for step in &window {
        if !distinct.iter().any(|seen| *seen == step.signature.as_str()) {
            distinct.push(step.signature.as_str());
            if distinct.len() > SUCCESS_STALL_DISTINCT_LIMIT {
                return None;
            }
        }
    }
    let every_observation_recurs = distinct.iter().all(|signature| {
        window
            .iter()
            .filter(|step| step.signature == *signature)
            .count()
            >= 2
    });
    every_observation_recurs.then_some(distinct.len())
}

fn success_progress_fingerprint(
    tool_name: &str,
    args: &Value,
    result: &ToolResultPayload,
) -> Option<(String, String)> {
    let canonical = resolve_tool_name(tool_name);
    if matches!(canonical.as_str(), "写入文件" | "write_file") {
        return write_progress_fingerprint(args, result);
    }
    if !matches!(canonical.as_str(), "执行命令" | "execute_command") {
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

/// Write-family progress fingerprint. The observation for a write is not stdout
/// but the real disk effect: same path, same requested content and the same
/// "nothing changed" counts mean the call cannot teach the model anything new.
/// A newly created file counts as progress even when it is empty.
fn write_progress_fingerprint(
    args: &Value,
    result: &ToolResultPayload,
) -> Option<(String, String)> {
    let change = super::tool_result_payload::file_write_change(&result.data)?;
    let content = args
        .as_object()
        .and_then(|object| object.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let detail = format!(
        "write:path={} added={} deleted={} hunks={} changed={}",
        change.path, change.added_lines, change.deleted_lines, change.hunks, change.changed
    );
    let fingerprint = format!(
        "path:{} content_hash:{:016x} added:{} deleted:{} hunks:{} changed:{}",
        change.path,
        hasher.finish(),
        change.added_lines,
        change.deleted_lines,
        change.hunks,
        change.changed
    );
    Some((
        normalize_detail(&fingerprint),
        detail.chars().take(FINGERPRINT_DETAIL_MAX_CHARS).collect(),
    ))
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
        MAX_SAME_SUCCESS_NO_PROGRESS, MAX_SUCCESS_NO_PROGRESS_WARNINGS,
        SUCCESS_STALL_NO_NEW_LINE_STREAK, SUCCESS_STALL_WINDOW,
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
            assert!(governor
                .record_failure("execute_command", &payload)
                .is_none());
        }
        let stop = governor
            .record_failure("execute_command", &payload)
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

    #[test]
    fn warns_then_stops_when_two_calls_alternate_without_progress() {
        let mut governor = SuccessProgressGovernor::default();
        let patch_result = ToolResultPayload {
            ok: true,
            data: json!({"results": [{"returncode": 0, "stdout": "1\n1"}]}),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        let verify_result = ToolResultPayload {
            ok: true,
            data: json!({"results": [{"returncode": 0, "stdout": "1\n1\npub fn duration"}]}),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        let mut first_warning = None;
        for index in 0..8 {
            let (args, result) = if index % 2 == 0 {
                (json!({"command": "patch turn_stats"}), &patch_result)
            } else {
                (json!({"command": "verify turn_stats"}), &verify_result)
            };
            if let Some(stop) = governor.record("execute_command", &args, result) {
                first_warning = Some(stop);
                break;
            }
        }
        let warning = first_warning.expect("alternating no-progress calls should be detected");
        assert_eq!(warning.distinct_steps, 2);
        assert!(
            !warning.escalated,
            "the first detection only warns the model"
        );
        assert_eq!(warning.threshold as usize, SUCCESS_STALL_WINDOW);

        // Ignoring the warning must not earn another warning: the second
        // detection stops the turn instead of letting the loop run to the
        // round budget.
        let mut escalation = None;
        for index in 0..8 {
            let (args, result) = if index % 2 == 0 {
                (json!({"command": "patch turn_stats"}), &patch_result)
            } else {
                (json!({"command": "verify turn_stats"}), &verify_result)
            };
            if let Some(stop) = governor.record("execute_command", &args, result) {
                escalation = Some(stop);
                break;
            }
        }
        assert!(
            escalation.expect("a relapse must stop the turn").escalated,
            "the second detection escalates to a hard stop"
        );
    }

    #[test]
    fn detects_a_loop_that_varies_the_command_tail() {
        // Shape of a real incident: the same patch script is re-run with a few
        // different verification tails, so no call ever repeats and up to four
        // observations rotate — but every new observation only reprints lines
        // seen before, and the file never changes.
        let mut governor = SuccessProgressGovernor::default();
        let script = "cd repo && python3 -c \"rewrite duration()\"";
        let tails = ["; grep -c fmt f", "; sed -n 1,5p f", "; od -c f", ""];
        let observations = [
            "1\n1",
            "1\n1\npub fn duration",
            "0000000 f o r m a t ! (",
            "1\n1\npub fn duration\nlet total = total",
        ];
        let mut detected = None;
        for index in 0..16 {
            let command = format!("{script}{}", tails[index % tails.len()]);
            let result = ToolResultPayload {
                ok: true,
                data: json!({"results": [{"returncode": 0, "stdout": observations[index % observations.len()]}]}),
                error: String::new(),
                sandbox: true,
                timestamp: Utc::now(),
                meta: None,
            };
            if let Some(stop) =
                governor.record("execute_command", &json!({"command": command}), &result)
            {
                detected = Some((index, stop));
                break;
            }
        }
        let (index, stop) = detected.expect("a recycled-output loop must be detected");
        // Four rotating observations each print one new line before the loop
        // settles, so detection lands a few calls after the streak threshold.
        assert!(
            index <= SUCCESS_STALL_NO_NEW_LINE_STREAK + 6,
            "detection should arrive shortly after the streak threshold, got index {index}"
        );
        assert_eq!(stop.threshold as usize, SUCCESS_STALL_NO_NEW_LINE_STREAK);
        assert!(!stop.escalated, "the first detection only warns");
    }

    #[test]
    fn a_newly_typed_stalled_pattern_only_warns_again() {
        let mut governor = SuccessProgressGovernor::default();
        let payload = ToolResultPayload {
            ok: true,
            data: json!({"results": [{"returncode": 0, "stdout": "no change"}]}),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        let run_cycle = |governor: &mut SuccessProgressGovernor, left: &str, right: &str| {
            for index in 0..8 {
                let command = if index % 2 == 0 { left } else { right };
                if let Some(stop) =
                    governor.record("execute_command", &json!({"command": command}), &payload)
                {
                    return Some(stop);
                }
            }
            None
        };

        let first = run_cycle(&mut governor, "patch-a", "verify-a").expect("first cycle detected");
        assert!(!first.escalated);
        // A different pair of calls is a different (still useless) approach: the
        // model gets a fresh warning instead of an immediate hard stop.
        let second =
            run_cycle(&mut governor, "patch-b", "verify-b").expect("second cycle detected");
        assert!(!second.escalated, "changed evidence must not escalate");
        // Relapsing into the first pattern is what proves the model ignored the
        // warning.
        let relapse = run_cycle(&mut governor, "patch-a", "verify-a").expect("relapse detected");
        assert!(
            !relapse.escalated,
            "the previous evidence changed in between, so the relapse warns once more"
        );
    }

    #[test]
    fn repeated_warnings_on_new_patterns_eventually_stop_the_turn() {
        let mut governor = SuccessProgressGovernor::default();
        let payload = ToolResultPayload {
            ok: true,
            data: json!({"results": [{"returncode": 0, "stdout": "no change"}]}),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        let mut escalations = 0;
        for cycle in 0..=MAX_SUCCESS_NO_PROGRESS_WARNINGS {
            let left = format!("patch-{}", cycle);
            let right = format!("verify-{}", cycle);
            for index in 0..8 {
                let command = if index % 2 == 0 { &left } else { &right };
                if let Some(stop) =
                    governor.record("execute_command", &json!({"command": command}), &payload)
                {
                    if stop.escalated {
                        escalations += 1;
                    }
                }
            }
        }
        assert_eq!(
            escalations, 1,
            "only the warning past the budget escalates, and only once"
        );
    }

    #[test]
    fn fresh_probe_mid_window_is_not_a_stalled_cycle() {
        let mut governor = SuccessProgressGovernor::default();
        let payload = ToolResultPayload {
            ok: true,
            data: json!({"results": [{"returncode": 0, "stdout": "identical"}]}),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        // Alternate two calls, then make a call whose observation is new: that is
        // still progress, so the stalled streak has to restart instead of
        // accumulating towards the threshold.
        let commands = ["probe-a", "probe-b", "probe-a", "probe-b", "probe-a"];
        for command in commands {
            assert!(governor
                .record("execute_command", &json!({"command": command}), &payload)
                .is_none());
        }
        let fresh = ToolResultPayload {
            ok: true,
            data: json!({"results": [{"returncode": 0, "stdout": "a line never seen before"}]}),
            error: String::new(),
            sandbox: true,
            timestamp: Utc::now(),
            meta: None,
        };
        assert!(governor
            .record(
                "execute_command",
                &json!({"command": "probe-fresh"}),
                &fresh
            )
            .is_none());
        assert!(governor
            .record("execute_command", &json!({"command": "probe-a"}), &payload)
            .is_none());
    }

    fn noop_write_payload() -> ToolResultPayload {
        ToolResultPayload {
            ok: true,
            data: json!({
                "action": "write_file",
                "state": "completed",
                "summary": "Updated file temp/fix.py.",
                "data": {
                    "path": "temp/fix.py",
                    "bytes": 12,
                    "dry_run": false,
                    "existed": true,
                    "previous_bytes": 12,
                    "added_lines": 0,
                    "deleted_lines": 0,
                    "changed_files": 1,
                    "hunks_applied": 0
                }
            }),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        }
    }

    #[test]
    fn stops_on_repeated_noop_write() {
        let mut governor = SuccessProgressGovernor::default();
        let payload = noop_write_payload();
        let args = json!({"path": "temp/fix.py", "content": "print('x')\n"});
        for _ in 0..(MAX_SAME_SUCCESS_NO_PROGRESS - 1) {
            assert!(governor.record("write_file", &args, &payload).is_none());
        }
        let stop = governor
            .record("write_file", &args, &payload)
            .expect("同一处空写重复到阈值必须告警");
        assert_eq!(stop.repeat_count, MAX_SAME_SUCCESS_NO_PROGRESS);
        assert!(!stop.escalated, "第一次只提示模型换策略");
        assert!(stop.detail.contains("changed=false"));
        // 告警后模型仍照原样重发：同样的证据复发，直接停轮。
        let mut escalation = None;
        for _ in 0..MAX_SAME_SUCCESS_NO_PROGRESS {
            if let Some(stop) = governor.record("write_file", &args, &payload) {
                escalation = Some(stop);
                break;
            }
        }
        assert!(
            escalation.expect("复发必须停轮").escalated,
            "重复空写在告警后复发要硬停"
        );
    }

    #[test]
    fn real_write_changes_are_progress() {
        let mut governor = SuccessProgressGovernor::default();
        for index in 1..=8u64 {
            let args = json!({"path": "temp/fix.py", "content": format!("line {index}\n")});
            let payload = ToolResultPayload {
                ok: true,
                data: json!({
                    "action": "write_file",
                    "state": "completed",
                    "summary": "Updated file temp/fix.py.",
                    "data": {
                        "path": "temp/fix.py",
                        "bytes": 8,
                        "dry_run": false,
                        "existed": true,
                        "previous_bytes": 8,
                        "added_lines": 1,
                        "deleted_lines": 1,
                        "changed_files": 1,
                        "hunks_applied": 1
                    }
                }),
                error: String::new(),
                sandbox: false,
                timestamp: Utc::now(),
                meta: None,
            };
            assert!(
                governor.record("write_file", &args, &payload).is_none(),
                "每次内容都真的变了，属于正常迭代"
            );
        }
    }

    #[test]
    fn write_with_unknown_diff_is_not_judged() {
        // dry_run 只校验、diff_unavailable 无法判定：都不能算「空写」，否则
        // 正常的预览/大文件写入会被误判成循环。
        let mut governor = SuccessProgressGovernor::default();
        let payload = ToolResultPayload {
            ok: true,
            data: json!({
                "action": "write_file",
                "state": "completed",
                "summary": "Validated write target for temp/fix.py without writing content.",
                "data": {
                    "path": "temp/fix.py",
                    "bytes": 12,
                    "dry_run": true,
                    "existed": true,
                    "previous_bytes": 12,
                    "added_lines": 0,
                    "deleted_lines": 0,
                    "changed_files": 1,
                    "hunks_applied": 0
                }
            }),
            error: String::new(),
            sandbox: false,
            timestamp: Utc::now(),
            meta: None,
        };
        let args = json!({"path": "temp/fix.py", "content": "print('x')\n"});
        for _ in 0..(MAX_SAME_SUCCESS_NO_PROGRESS * 2) {
            assert!(governor.record("write_file", &args, &payload).is_none());
        }
    }
}
