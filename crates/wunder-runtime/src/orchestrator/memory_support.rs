use super::*;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use uuid::Uuid;

pub(super) use super::memory_auto_extract::*;
pub(super) use super::memory_compaction_window::*;

pub(super) const COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS: i64 = 64;
pub(super) const COMPACTION_RETAINED_INTERACTION_EXCHANGE_COUNT_PER_SIDE: usize = 2;
pub(super) const COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE: usize =
    COMPACTION_RETAINED_INTERACTION_EXCHANGE_COUNT_PER_SIDE * 2;
pub(super) const COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS: i64 = 8_192;
pub(super) const COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS: i64 = 16_384;
pub(super) const COMPACTION_RETAINED_INTERACTION_MESSAGE_MAX_TOKENS: i64 = 5_120;
pub(super) const COMPACTION_RETAINED_INTERACTION_TURN_MAX_CHARS: usize = 20_000;
pub(super) const PROMPT_MEMORY_RECALL_LIMIT: usize = 30;
pub(super) const COMPACTION_INFLIGHT_CURRENT_USER_META_KEY: &str =
    "compaction_inflight_current_user";
pub(super) const COMPACTION_RETAINED_INTERACTION_META_KEY: &str = "compaction_retained_interaction";
pub(super) const COMPACTION_SUMMARY_REASONING_EFFORT: &str = "none";
pub(super) const COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS: i64 = 256;
pub(super) const COMPACTION_TEXT_TRUNCATION_SUFFIX: &str = "...(truncated)";
pub(super) const COMPACTION_SUMMARY_MAX_CHARS: usize = 20_000;
pub(super) const COMPACTION_MIN_SUMMARY_MEANINGFUL_CHARS: usize = 8;
pub(super) const COMPACTION_DEBUG_PREVIEW_CHARS: usize = 240;
pub(super) const COMPACTION_MIN_RETAINED_INTERACTION_TOKENS: i64 = 128;
pub(super) const COMPACTION_CURRENT_TURN_FINAL_NOTE: &str =
    "The compaction summary indicates that the current user-facing task has no remaining executable work, or only needs a final/clarifying response. Treat retained user messages as historical context, not as a fresh request. Do not call tools, regenerate artifacts, rewrite files, or restart the original request solely because older messages are visible. Provide the final response now from the compaction summary and retained latest messages.";
pub(super) const COMPACTION_CURRENT_TURN_SUCCESS_NOTE: &str =
    "The current user request already produced successful tool output before compaction, but that does not by itself mean the user-facing task is complete. Continue from the latest retained tool observation and the compaction summary. Treat retained user messages as historical context, not as a fresh request. If the summary resume action is final or ask_user, do not call tools; answer or ask the user now. If the resume action is continue, perform only the named remaining step. Do not restart or redo completed work.";
pub(super) const COMPACTION_CURRENT_TURN_REPAIR_NOTE: &str =
    "[Compaction continuation]\nThe current user request already entered tool execution and the latest tool output reports failure. Do not restart from the original user request. Continue from the retained failure observation, change strategy, and repair or explain based on the available evidence.";

#[derive(Debug, Default)]
pub(super) struct RebuiltContextGuardStats {
    pub(super) applied: bool,
    pub(super) tokens_before: i64,
    pub(super) tokens_after: i64,
    pub(super) current_user_trimmed: bool,
    pub(super) current_user_tokens_before: i64,
    pub(super) current_user_tokens_after: i64,
    pub(super) summary_trimmed: bool,
    pub(super) summary_tokens_before: i64,
    pub(super) summary_tokens_after: i64,
    pub(super) summary_removed: bool,
    pub(super) fallback_trim_applied: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CurrentTurnProgressState {
    #[default]
    Pending,
    ToolSucceeded,
    ToolFailed,
    InProgress,
}

impl CurrentTurnProgressState {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::ToolSucceeded => "tool_succeeded",
            Self::ToolFailed => "tool_failed",
            Self::InProgress => "in_progress",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct CurrentTurnProgress {
    pub(super) state: CurrentTurnProgressState,
    pub(super) has_post_user_messages: bool,
    pub(super) has_tool_success: bool,
    pub(super) has_tool_failure: bool,
    pub(super) latest_tool_observation: Option<ToolObservationSnapshot>,
}

impl CurrentTurnProgress {
    pub(super) fn state_label(&self) -> &'static str {
        self.state.label()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CurrentUserReplayMode {
    Original,
    Placeholder,
    FinalContinuation,
    ToolSuccessContinuation,
    RepairContinuation,
    Omitted,
}

impl CurrentUserReplayMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Placeholder => "placeholder",
            Self::FinalContinuation => "final_continuation",
            Self::ToolSuccessContinuation => "tool_success_continuation",
            Self::RepairContinuation => "repair_continuation",
            Self::Omitted => "omitted",
        }
    }
}

#[derive(Debug)]
pub(super) struct CurrentUserReplay {
    pub(super) message: Option<Value>,
    pub(super) mode: CurrentUserReplayMode,
    pub(super) trimmed: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct ToolObservationSnapshot {
    pub(super) tool_name: String,
    pub(super) ok: Option<bool>,
    pub(super) summary: String,
    pub(super) next_step_hint: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum CompactionResumeAction {
    Final,
    Continue,
    Retry,
    AskUser,
    #[default]
    Unknown,
}

impl CompactionResumeAction {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Final => "final",
            Self::Continue => "continue",
            Self::Retry => "retry",
            Self::AskUser => "ask_user",
            Self::Unknown => "unknown",
        }
    }

    pub(super) fn should_finalize(self) -> bool {
        matches!(self, Self::Final | Self::AskUser)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CompactionRunMode {
    Manual,
    AutoLoop,
    OverflowRecovery,
}

impl CompactionRunMode {
    pub(super) fn trigger_mode(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AutoLoop => "auto_loop",
            Self::OverflowRecovery => "overflow_recovery",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CompactionExecutionProfile {
    pub(super) run_mode: CompactionRunMode,
    pub(super) prefer_preserving_summary: bool,
}

impl CompactionExecutionProfile {
    pub(super) fn new(run_mode: CompactionRunMode) -> Self {
        Self {
            run_mode,
            prefer_preserving_summary: true,
        }
    }

    pub(super) fn trigger_mode(self) -> &'static str {
        self.run_mode.trigger_mode()
    }
}

pub(super) fn insert_compaction_trigger_mode(
    payload: &mut serde_json::Map<String, Value>,
    trigger_mode: Option<&str>,
) {
    let Some(trigger_mode) = trigger_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    payload
        .entry("trigger_mode".to_string())
        .or_insert_with(|| Value::String(trigger_mode.to_string()));
}

pub(super) fn insert_compaction_id(
    payload: &mut serde_json::Map<String, Value>,
    compaction_id: Option<&str>,
) {
    let Some(compaction_id) = compaction_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    payload
        .entry("compaction_id".to_string())
        .or_insert_with(|| Value::String(compaction_id.to_string()));
}

pub(super) fn new_compaction_id(run_mode: CompactionRunMode) -> String {
    format!(
        "cmp_{}_{}",
        run_mode.trigger_mode(),
        Uuid::new_v4().simple()
    )
}

#[derive(Debug)]
pub(super) struct CompactionResult {
    pub(super) messages: Vec<Value>,
    pub(super) compaction_id: Option<String>,
}

impl CompactionResult {
    pub(super) fn unchanged(messages: Vec<Value>) -> Self {
        Self {
            messages,
            compaction_id: None,
        }
    }

    pub(super) fn compacted(messages: Vec<Value>, compaction_id: String) -> Self {
        Self {
            messages,
            compaction_id: Some(compaction_id),
        }
    }
}

pub(super) fn reduce_to_summary_priority_context(
    messages: &mut Vec<Value>,
    limit: i64,
    stats: &mut RebuiltContextGuardStats,
) {
    let summary_index = locate_compaction_summary_message_index(messages);
    let current_user_index = locate_rebuilt_current_user_index(messages);

    let mut prioritized = Vec::new();
    if let Some(system_message) = messages
        .first()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .cloned()
    {
        prioritized.push(system_message);
    }
    if let Some(summary_index) = summary_index {
        prioritized.extend(
            messages[..summary_index]
                .iter()
                .filter(|message| is_retained_interaction_message(message))
                .cloned(),
        );
        prioritized.push(messages[summary_index].clone());
    }
    if let Some(summary_index) = summary_index {
        let tail_end = current_user_index.unwrap_or(messages.len());
        if summary_index + 1 < tail_end {
            prioritized.extend(
                messages[summary_index + 1..tail_end]
                    .iter()
                    .filter(|message| is_retained_interaction_message(message))
                    .cloned(),
            );
        }
    }
    if let Some(current_user_index) = current_user_index {
        if Some(current_user_index) != summary_index {
            prioritized.push(messages[current_user_index].clone());
        }
    }

    if prioritized.is_empty() {
        return;
    }

    *messages = prioritized;
    let mut total_tokens = estimate_messages_tokens(messages);

    if total_tokens > limit {
        let current_user_index = locate_rebuilt_current_user_index(messages);
        let summary_index = locate_compaction_summary_message_index(messages);
        if let (Some(summary_index), Some(current_user_index)) = (summary_index, current_user_index)
        {
            if current_user_index != summary_index {
                stats.current_user_tokens_before = stats
                    .current_user_tokens_before
                    .max(estimate_message_tokens(&messages[current_user_index]));
                let remaining_for_current =
                    (limit - (total_tokens - stats.current_user_tokens_before)).max(1);
                if let Some(trimmed) =
                    trim_message_to_fit_tokens(&messages[current_user_index], remaining_for_current)
                {
                    stats.current_user_tokens_after = estimate_message_tokens(&trimmed);
                    stats.current_user_trimmed = stats.current_user_trimmed
                        || stats.current_user_tokens_after < stats.current_user_tokens_before;
                    messages[current_user_index] = trimmed;
                    total_tokens = estimate_messages_tokens(messages);
                }
            }
        }
    }

    if total_tokens > limit {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = stats
                .summary_tokens_before
                .max(estimate_message_tokens(&messages[summary_index]));
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) = trim_compaction_summary_message_to_fit_tokens(
                &messages[summary_index],
                remaining_for_summary,
            ) {
                let trimmed_summary =
                    extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
                if !is_invalid_compaction_summary(&trimmed_summary) {
                    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                    stats.summary_trimmed = stats.summary_trimmed
                        || stats.summary_tokens_after < stats.summary_tokens_before;
                    messages[summary_index] = trimmed;
                }
            }
        }
    }

    if total_tokens > limit {
        rebalance_retained_interaction_context(messages, limit);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        trim_summary_to_preserve_retained_interaction_budget(messages, limit, stats);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        if tighten_retained_interaction_context(messages, limit) {
            total_tokens = estimate_messages_tokens(messages);
        }
    }

    if total_tokens > limit {
        let current_user_index = locate_rebuilt_current_user_index(messages);
        let summary_index = locate_compaction_summary_message_index(messages);
        *messages = messages
            .iter()
            .enumerate()
            .filter_map(|(index, message)| {
                if message.get("role").and_then(Value::as_str) == Some("system")
                    || Some(index) == summary_index
                    || Some(index) == current_user_index
                {
                    Some(message.clone())
                } else {
                    None
                }
            })
            .collect();
    }
}

pub(super) fn trim_summary_to_preserve_retained_interaction_budget(
    messages: &mut Vec<Value>,
    limit: i64,
    stats: &mut RebuiltContextGuardStats,
) {
    if messages.is_empty() || limit <= 0 {
        return;
    }

    let retained_total_tokens = messages
        .iter()
        .filter(|message| is_retained_interaction_message(message))
        .map(estimate_message_tokens)
        .sum::<i64>();
    if retained_total_tokens <= 0 {
        return;
    }

    let total_tokens = estimate_messages_tokens(messages);
    if total_tokens <= limit {
        return;
    }

    let retained_floor = retained_total_tokens.min(COMPACTION_MIN_RETAINED_INTERACTION_TOKENS);
    let reducible_retained_tokens = retained_total_tokens.saturating_sub(retained_floor);
    let overflow = total_tokens - limit;
    if overflow <= reducible_retained_tokens {
        return;
    }

    let Some(summary_index) = locate_compaction_summary_message_index(messages) else {
        return;
    };
    let summary_tokens_before = estimate_message_tokens(&messages[summary_index]);
    if summary_tokens_before <= 1 {
        return;
    }

    let required_summary_reduction = overflow - reducible_retained_tokens;
    let target_tokens = summary_tokens_before
        .saturating_sub(required_summary_reduction)
        .max(1);
    let Some(trimmed) =
        trim_compaction_summary_message_to_fit_tokens(&messages[summary_index], target_tokens)
    else {
        return;
    };
    let trimmed_summary =
        extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
    if is_invalid_compaction_summary(&trimmed_summary) {
        return;
    }

    stats.summary_tokens_before = stats.summary_tokens_before.max(summary_tokens_before);
    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
    stats.summary_trimmed |= stats.summary_tokens_after < stats.summary_tokens_before;
    messages[summary_index] = trimmed;
}

pub(super) fn rebalance_retained_interaction_context(messages: &mut Vec<Value>, limit: i64) {
    if messages.is_empty() || limit <= 0 {
        return;
    }

    let summary_index = locate_compaction_summary_message_index(messages);
    let current_user_index = locate_rebuilt_current_user_index(messages);
    let preserved_tokens = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            !is_retained_interaction_message(message)
                || Some(*index) == summary_index
                || Some(*index) == current_user_index
        })
        .map(|(_, message)| estimate_message_tokens(message))
        .sum::<i64>();
    let remaining = limit.saturating_sub(preserved_tokens);

    let head_messages = summary_index
        .map(|summary_index| {
            messages[..summary_index]
                .iter()
                .filter(|message| is_retained_interaction_message(message))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tail_messages = match summary_index {
        Some(summary_index) => {
            let tail_end = current_user_index.unwrap_or(messages.len());
            if summary_index + 1 >= tail_end {
                Vec::new()
            } else {
                messages[summary_index + 1..tail_end]
                    .iter()
                    .filter(|message| is_retained_interaction_message(message))
                    .cloned()
                    .collect::<Vec<_>>()
            }
        }
        None => Vec::new(),
    };

    let head_tokens_total = estimate_messages_tokens(&head_messages);
    let tail_tokens_total = estimate_messages_tokens(&tail_messages);
    let total_tokens = head_tokens_total.saturating_add(tail_tokens_total);
    let (head_budget, tail_budget) = if remaining <= 0 || total_tokens <= 0 {
        (0, 0)
    } else if total_tokens <= remaining {
        (head_tokens_total, tail_tokens_total)
    } else {
        let mut head_budget = remaining
            .saturating_mul(head_tokens_total)
            .checked_div(total_tokens)
            .unwrap_or(0);
        if head_tokens_total > 0 && head_budget == 0 {
            head_budget = 1;
        }
        let mut tail_budget = remaining.saturating_sub(head_budget);
        if tail_tokens_total > 0 && tail_budget == 0 && remaining > 1 {
            tail_budget = 1;
            head_budget = remaining.saturating_sub(1);
        }
        (head_budget, tail_budget)
    };
    let retained_head =
        collect_retained_interaction_messages_from_window(&head_messages, head_budget, false);
    let retained_tail =
        collect_retained_interaction_messages_from_window(&tail_messages, tail_budget, true);

    let system_message = messages
        .first()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .cloned();
    let summary_message = summary_index.and_then(|index| messages.get(index)).cloned();
    let current_user_message = current_user_index
        .and_then(|index| messages.get(index))
        .cloned();

    let mut rebuilt = Vec::new();
    if let Some(system_message) = system_message {
        rebuilt.push(system_message);
    }
    rebuilt.extend(retained_head);
    if let Some(summary_message) = summary_message {
        rebuilt.push(summary_message);
    }
    rebuilt.extend(retained_tail);
    if let Some(current_user_message) = current_user_message {
        if !is_compaction_inflight_current_user_message(&current_user_message)
            || rebuilt.last() != Some(&current_user_message)
        {
            rebuilt.push(current_user_message);
        }
    }
    *messages = rebuilt;
}

pub(super) fn tighten_retained_interaction_context(messages: &mut Vec<Value>, limit: i64) -> bool {
    if messages.is_empty() || limit <= 0 {
        return false;
    }

    let mut changed = false;
    loop {
        let total_tokens = estimate_messages_tokens(messages);
        if total_tokens <= limit {
            break;
        }
        let overflow = total_tokens - limit;
        let retained_candidate = messages
            .iter()
            .enumerate()
            .filter(|(_, message)| is_retained_interaction_message(message))
            .max_by_key(|(_, message)| estimate_message_tokens(message))
            .map(|(index, message)| (index, estimate_message_tokens(message)));
        let Some((index, retained_tokens)) = retained_candidate else {
            break;
        };
        if retained_tokens <= 1 {
            messages.remove(index);
            changed = true;
            continue;
        }

        let target_tokens =
            (retained_tokens - overflow).clamp(1, retained_tokens.saturating_sub(1));
        let trimmed = trim_message_to_fit_tokens(&messages[index], target_tokens);
        let next_message =
            trimmed.filter(|candidate| estimate_message_tokens(candidate) < retained_tokens);

        if let Some(next_message) = next_message {
            messages[index] = next_message;
        } else {
            messages.remove(index);
        }
        changed = true;
    }

    changed
}

pub(super) fn apply_rebuilt_context_guard(
    messages: &mut Vec<Value>,
    limit: i64,
    prefer_preserving_summary: bool,
) -> RebuiltContextGuardStats {
    let mut stats = RebuiltContextGuardStats {
        tokens_before: estimate_messages_tokens(messages),
        ..Default::default()
    };
    if limit <= 0 || stats.tokens_before <= limit || messages.is_empty() {
        stats.tokens_after = stats.tokens_before;
        return stats;
    }

    stats.applied = true;

    let mut total_tokens = estimate_messages_tokens(messages);

    if total_tokens > limit && !prefer_preserving_summary {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = estimate_message_tokens(&messages[summary_index]);
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) = trim_compaction_summary_message_to_fit_tokens(
                &messages[summary_index],
                remaining_for_summary,
            ) {
                let trimmed_summary =
                    extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
                if !is_invalid_compaction_summary(&trimmed_summary) {
                    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                    stats.summary_trimmed =
                        stats.summary_tokens_after < stats.summary_tokens_before;
                    messages[summary_index] = trimmed;
                } else {
                    stats.summary_tokens_after = stats.summary_tokens_before;
                }
            } else {
                stats.summary_tokens_after = stats.summary_tokens_before;
            }
        }
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        loop {
            let summary_index = locate_compaction_summary_message_index(messages);
            let current_user_index = locate_rebuilt_current_user_index(messages);
            let removable_index = messages.iter().enumerate().find_map(|(index, message)| {
                if Some(index) == summary_index || Some(index) == current_user_index {
                    return None;
                }
                if message.get("role").and_then(Value::as_str) == Some("system") {
                    return None;
                }
                if is_retained_interaction_message(message) {
                    return None;
                }
                Some(index)
            });
            let Some(index) = removable_index else {
                break;
            };
            messages.remove(index);
            total_tokens = estimate_messages_tokens(messages);
            if total_tokens <= limit {
                break;
            }
        }
    }

    if total_tokens > limit && prefer_preserving_summary {
        trim_summary_to_preserve_retained_interaction_budget(messages, limit, &mut stats);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        rebalance_retained_interaction_context(messages, limit);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        if tighten_retained_interaction_context(messages, limit) {
            total_tokens = estimate_messages_tokens(messages);
        }
    }

    if total_tokens > limit && !prefer_preserving_summary {
        let summary_index = locate_compaction_summary_message_index(messages);
        let current_user_index = locate_rebuilt_current_user_index(messages);
        if let (Some(summary_index), Some(current_user_index)) = (summary_index, current_user_index)
        {
            if summary_index != current_user_index && summary_index < messages.len() {
                messages.remove(summary_index);
                stats.summary_removed = true;
                total_tokens = estimate_messages_tokens(messages);
            }
        }
    }

    if total_tokens > limit {
        let summary_index = locate_compaction_summary_message_index(messages);
        let current_user_index = locate_rebuilt_current_user_index(messages);
        if let (Some(summary_index), Some(current_user_index)) = (summary_index, current_user_index)
        {
            if current_user_index != summary_index {
                stats.current_user_tokens_before =
                    estimate_message_tokens(&messages[current_user_index]);
                let preserve_floor = stats
                    .current_user_tokens_before
                    .clamp(1, COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS);
                let remaining_for_current =
                    limit - (total_tokens - stats.current_user_tokens_before);
                let target_tokens = remaining_for_current
                    .max(preserve_floor)
                    .min(stats.current_user_tokens_before);
                // Keep the active user intent readable whenever the limit still allows it.
                if target_tokens < stats.current_user_tokens_before {
                    if let Some(trimmed) =
                        trim_message_to_fit_tokens(&messages[current_user_index], target_tokens)
                    {
                        stats.current_user_tokens_after = estimate_message_tokens(&trimmed);
                        stats.current_user_trimmed =
                            stats.current_user_tokens_after < stats.current_user_tokens_before;
                        messages[current_user_index] = trimmed;
                        total_tokens = estimate_messages_tokens(messages);
                    } else {
                        stats.current_user_tokens_after = stats.current_user_tokens_before;
                    }
                } else {
                    stats.current_user_tokens_after = stats.current_user_tokens_before;
                }
            }
        }
    }

    if total_tokens > limit && prefer_preserving_summary {
        if let Some(summary_index) = locate_compaction_summary_message_index(messages) {
            stats.summary_tokens_before = stats
                .summary_tokens_before
                .max(estimate_message_tokens(&messages[summary_index]));
            let remaining_for_summary =
                (limit - (total_tokens - stats.summary_tokens_before)).max(1);
            if let Some(trimmed) = trim_compaction_summary_message_to_fit_tokens(
                &messages[summary_index],
                remaining_for_summary,
            ) {
                let trimmed_summary =
                    extract_guard_content_text(trimmed.get("content").unwrap_or(&Value::Null));
                if !is_invalid_compaction_summary(&trimmed_summary) {
                    stats.summary_tokens_after = estimate_message_tokens(&trimmed);
                    stats.summary_trimmed = stats.summary_trimmed
                        || stats.summary_tokens_after < stats.summary_tokens_before;
                    messages[summary_index] = trimmed;
                    total_tokens = estimate_messages_tokens(messages);
                }
            }
        }
    }

    if total_tokens > limit {
        if let Some(last_index) = messages.len().checked_sub(1) {
            let last_tokens = estimate_message_tokens(&messages[last_index]);
            let current_user_index = locate_rebuilt_current_user_index(messages);
            let trimming_current_user = current_user_index == Some(last_index);
            let trimming_summary =
                locate_compaction_summary_message_index(messages) == Some(last_index);
            if trimming_current_user && stats.current_user_tokens_before == 0 {
                stats.current_user_tokens_before = last_tokens;
            }
            let remaining_for_last = (limit - (total_tokens - last_tokens)).max(1);
            let trimmed = if trimming_summary {
                trim_compaction_summary_message_to_fit_tokens(
                    &messages[last_index],
                    remaining_for_last,
                )
            } else {
                trim_message_to_fit_tokens(&messages[last_index], remaining_for_last)
            };
            if let Some(trimmed) = trimmed {
                let trimmed_tokens = estimate_message_tokens(&trimmed);
                if trimming_current_user {
                    stats.current_user_tokens_after = trimmed_tokens;
                    stats.current_user_trimmed |= trimmed_tokens < stats.current_user_tokens_before;
                }
                messages[last_index] = trimmed;
                total_tokens = estimate_messages_tokens(messages);
            } else if trimming_summary && last_tokens > remaining_for_last {
                messages.remove(last_index);
                stats.summary_removed = true;
                total_tokens = estimate_messages_tokens(messages);
            }
        }
    }

    if total_tokens > limit && prefer_preserving_summary {
        reduce_to_summary_priority_context(messages, limit, &mut stats);
        total_tokens = estimate_messages_tokens(messages);
    }

    if total_tokens > limit {
        *messages = trim_messages_to_budget(messages, limit);
        stats.fallback_trim_applied = true;
        total_tokens = estimate_messages_tokens(messages);
        if total_tokens > limit {
            if let Some(last_index) = messages.len().checked_sub(1) {
                let trimming_summary =
                    locate_compaction_summary_message_index(messages) == Some(last_index);
                let trimmed = if trimming_summary {
                    trim_compaction_summary_message_to_fit_tokens(
                        &messages[last_index],
                        limit.max(1),
                    )
                } else {
                    trim_message_to_fit_tokens(&messages[last_index], limit.max(1))
                };
                if let Some(trimmed) = trimmed {
                    *messages = vec![trimmed];
                    total_tokens = estimate_messages_tokens(messages);
                } else if trimming_summary {
                    messages.clear();
                    total_tokens = 0;
                }
            }
        }
    }

    stats.tokens_after = total_tokens;
    stats
}

pub(super) fn trim_message_to_fit_tokens(message: &Value, max_tokens: i64) -> Option<Value> {
    if max_tokens <= 0 || estimate_message_tokens(message) <= max_tokens {
        return None;
    }
    let mut message_obj = message.as_object().cloned().unwrap_or_else(|| {
        let mut fallback = serde_json::Map::new();
        fallback.insert("role".to_string(), Value::String("user".to_string()));
        fallback.insert("content".to_string(), message.clone());
        fallback
    });
    let source = extract_guard_content_text(message_obj.get("content").unwrap_or(&Value::Null));
    let source = if source.trim().is_empty() {
        i18n::t("compaction.summary_fallback")
    } else {
        source
    };
    let mut target_tokens = max_tokens.max(1);
    let mut trimmed_message: Option<Value> = None;
    for _ in 0..4 {
        let content =
            trim_text_to_tokens(&source, target_tokens, COMPACTION_TEXT_TRUNCATION_SUFFIX);
        message_obj.insert("content".to_string(), Value::String(content));
        message_obj.remove("reasoning_content");
        message_obj.remove("reasoning");
        let candidate = Value::Object(message_obj.clone());
        let cost = estimate_message_tokens(&candidate);
        trimmed_message = Some(candidate.clone());
        if cost <= max_tokens {
            break;
        }
        let overflow = cost - max_tokens;
        let next_target = (target_tokens - overflow).max(1);
        if next_target == target_tokens {
            break;
        }
        target_tokens = next_target;
    }
    trimmed_message
}

pub(super) fn trim_compaction_summary_message_to_fit_tokens(
    message: &Value,
    max_tokens: i64,
) -> Option<Value> {
    if max_tokens <= 0 || estimate_message_tokens(message) <= max_tokens {
        return None;
    }

    let summary_text = extract_guard_content_text(message.get("content").unwrap_or(&Value::Null));
    if !starts_with_compaction_prefix(&summary_text) {
        return trim_message_to_fit_tokens(message, max_tokens);
    }

    let mut message_obj = message.as_object().cloned().unwrap_or_else(|| {
        let mut fallback = serde_json::Map::new();
        fallback.insert("role".to_string(), Value::String("user".to_string()));
        fallback.insert("content".to_string(), Value::String(summary_text.clone()));
        fallback
    });

    let prefix = i18n::t("history.compaction_prefix");
    let minimum_chars = prefix
        .chars()
        .count()
        .saturating_add(1)
        .saturating_add(COMPACTION_MIN_SUMMARY_MEANINGFUL_CHARS);
    let mut target_chars = ((max_tokens.max(1) as f64) * 4.0).ceil() as usize;
    target_chars = target_chars.max(minimum_chars);

    for _ in 0..4 {
        let content = clamp_committed_compaction_summary(&summary_text, target_chars);
        if is_invalid_compaction_summary(&content) {
            return None;
        }
        message_obj.insert("content".to_string(), Value::String(content));
        message_obj.remove("reasoning_content");
        message_obj.remove("reasoning");
        let candidate = Value::Object(message_obj.clone());
        let cost = estimate_message_tokens(&candidate);
        if cost <= max_tokens {
            return Some(candidate);
        }
        let overflow_chars = ((cost - max_tokens).max(1) as f64 * 4.0).ceil() as usize;
        let next_target = target_chars
            .saturating_sub(overflow_chars)
            .max(minimum_chars);
        if next_target == target_chars {
            break;
        }
        target_chars = next_target;
    }

    None
}

pub(super) fn extract_guard_content_text(content: &Value) -> String {
    extract_memory_summary_text_value(content)
}

pub(super) fn extract_memory_summary_text_value(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(text) => strip_tool_calls(text).trim().to_string(),
        Value::Array(parts) => {
            let mut segments = Vec::new();
            for part in parts {
                let Some(obj) = part.as_object() else {
                    continue;
                };
                let part_type = obj
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                if part_type == "text" {
                    let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
                    let cleaned = strip_tool_calls(text).trim().to_string();
                    if !cleaned.is_empty() {
                        segments.push(cleaned);
                    }
                } else if part_type == "image_url" || obj.contains_key("image_url") {
                    segments.push(i18n::t("memory.summary.image_placeholder"));
                } else if let Some(text) = obj.get("text").and_then(Value::as_str) {
                    let cleaned = strip_tool_calls(text).trim().to_string();
                    if !cleaned.is_empty() {
                        segments.push(cleaned);
                    }
                }
            }
            segments.join("\n").trim().to_string()
        }
        other => strip_tool_calls(&other.to_string()).trim().to_string(),
    }
}

pub(super) fn prepare_compaction_summary_messages(
    messages: Vec<Value>,
    max_tokens: i64,
) -> Vec<Value> {
    if messages.is_empty() {
        return messages;
    }
    let target = max_tokens.max(1);
    let mut prepared = Vec::with_capacity(messages.len());
    let mut merged_system_blocks: Vec<String> = Vec::new();
    for message in messages {
        let Some(obj) = message.as_object() else {
            continue;
        };
        let role = obj
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if role.is_empty() {
            continue;
        }
        let Some(mut content) = extract_compaction_summary_message_text(role.as_str(), obj) else {
            continue;
        };
        let per_message_target = if is_compaction_observation_message(role.as_str(), obj) {
            target.min(COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS)
        } else {
            target
        };
        if approx_token_count(&content) > per_message_target {
            content = trim_text_to_tokens(
                &content,
                per_message_target,
                COMPACTION_TEXT_TRUNCATION_SUFFIX,
            );
        }
        let normalized_role = normalize_compaction_summary_role(role.as_str());
        if normalized_role == "system" {
            merged_system_blocks.push(content);
            continue;
        }
        prepared.push(json!({
            "role": normalized_role,
            "content": content,
        }));
    }
    if !merged_system_blocks.is_empty() {
        let merged = merged_system_blocks.join("\n\n");
        let merged = if approx_token_count(&merged) > target {
            trim_text_to_tokens(&merged, target, COMPACTION_TEXT_TRUNCATION_SUFFIX)
        } else {
            merged
        };
        prepared.insert(0, json!({ "role": "system", "content": merged }));
    }
    prepared
}

pub(super) fn build_compaction_summary_input(
    system_message: Option<&Value>,
    source_messages: &[Value],
    compaction_message: Value,
) -> Vec<Value> {
    let mut summary_input = Vec::with_capacity(
        source_messages
            .len()
            .saturating_add(usize::from(system_message.is_some()))
            .saturating_add(1),
    );
    if let Some(system_message) = system_message {
        summary_input.push(system_message.clone());
    }
    summary_input.extend(source_messages.iter().cloned());
    // Keep the compaction instruction as the final model-visible item. Tool
    // failures after the current user request can otherwise override the
    // summarization task and make the model continue normal work instead.
    summary_input.push(compaction_message);
    summary_input
}

pub(super) fn normalize_compaction_summary_role(role: &str) -> &'static str {
    match role {
        "system" => "system",
        "assistant" => "assistant",
        _ => "user",
    }
}

pub(super) fn extract_compaction_summary_message_text(
    role: &str,
    obj: &Map<String, Value>,
) -> Option<String> {
    let content = obj.get("content").unwrap_or(&Value::Null);
    let text = if is_compaction_observation_message(role, obj) {
        summarize_compaction_observation(content)
    } else {
        strip_compaction_internal_tool_lines(&extract_memory_summary_text_value(content))
    };
    if !text.is_empty() {
        return Some(text);
    }
    None
}

pub(super) fn is_compaction_observation_message(role: &str, obj: &Map<String, Value>) -> bool {
    let content = obj.get("content").unwrap_or(&Value::Null);
    Orchestrator::is_observation_message(role, content)
}

pub(super) fn is_tool_role_message(message: &Value) -> bool {
    message.get("role").and_then(Value::as_str) == Some("tool")
}

pub(super) fn has_tool_call_payload(message: &Value) -> bool {
    message
        .get("tool_calls")
        .or_else(|| message.get("toolCalls"))
        .or_else(|| message.get("function_call"))
        .or_else(|| message.get("functionCall"))
        .is_some_and(|value| !value.is_null())
}

pub(super) fn classify_current_turn_progress(
    messages: &[Value],
    current_user_index: usize,
) -> CurrentTurnProgress {
    let mut progress = CurrentTurnProgress::default();
    let trailing = messages
        .get(current_user_index.saturating_add(1)..)
        .unwrap_or(&[]);
    progress.has_post_user_messages = !trailing.is_empty();
    for message in trailing {
        let Some(obj) = message.as_object() else {
            continue;
        };
        let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
        let content = obj.get("content").unwrap_or(&Value::Null);
        if is_tool_role_message(message) || Orchestrator::is_observation_message(role, content) {
            let observation = parse_compaction_observation_payload(content);
            match observation
                .as_ref()
                .and_then(|payload| payload.get("ok"))
                .and_then(Value::as_bool)
            {
                Some(true) => progress.has_tool_success = true,
                Some(false) => progress.has_tool_failure = true,
                None => progress.has_post_user_messages = true,
            }
            if let Some(snapshot) = observation
                .as_ref()
                .and_then(build_tool_observation_snapshot)
            {
                progress.latest_tool_observation = Some(snapshot);
            }
            continue;
        }
        if role == "assistant" && has_tool_call_payload(message) {
            progress.has_post_user_messages = true;
        }
    }
    progress.state = if progress.has_tool_failure {
        CurrentTurnProgressState::ToolFailed
    } else if progress.has_tool_success {
        CurrentTurnProgressState::ToolSucceeded
    } else if progress.has_post_user_messages {
        CurrentTurnProgressState::InProgress
    } else {
        CurrentTurnProgressState::Pending
    };
    progress
}

pub(super) fn build_tool_observation_snapshot(payload: &Value) -> Option<ToolObservationSnapshot> {
    let map = payload.as_object()?;
    let tool_name = map
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let ok = map.get("ok").and_then(Value::as_bool);
    let summary = summarize_compaction_observation_payload(payload);
    let next_step_hint = infer_next_step_after_successful_tool(&tool_name, map);
    Some(ToolObservationSnapshot {
        tool_name,
        ok,
        summary,
        next_step_hint,
    })
}

pub(super) fn infer_next_step_after_successful_tool(
    tool_name: &str,
    payload: &Map<String, Value>,
) -> Option<String> {
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    if matches!(
        tool_name.trim(),
        "写入文件" | "write_file" | "文本编辑" | "edit_file2"
    ) {
        let data = payload.get("data").and_then(Value::as_object)?;
        let path = data
            .get("path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        return Some(format!(
            "A successful file write is usually an intermediate step. Continue with the next action required by the summary, such as running or validating `{path}`, before finalizing."
        ));
    }
    if matches!(
        tool_name.trim(),
        "读取文件" | "read_file" | "搜索内容" | "search_content" | "列出文件" | "list_files"
    ) {
        return Some(
            "The latest successful tool only gathered information. Use it to decide the next concrete action; do not treat the task as complete unless the summary says no deliverable remains."
                .to_string(),
        );
    }
    None
}

pub(super) fn append_compaction_summary_excerpt(note: &mut String, summary_text: &str) {
    let body = strip_compaction_prefix_text(summary_text);
    let excerpt = trim_text_to_chars(body.trim(), 1_200, COMPACTION_TEXT_TRUNCATION_SUFFIX);
    if excerpt.trim().is_empty() {
        return;
    }
    note.push('\n');
    note.push_str("- summary_excerpt:\n");
    note.push_str(&excerpt);
}

pub(super) fn detect_compaction_resume_action(summary_text: &str) -> CompactionResumeAction {
    let body = strip_compaction_prefix_text(summary_text);
    let normalized = body.replace("\r\n", "\n");
    for line in normalized.lines() {
        let trimmed = line.trim().trim_start_matches(['-', '*', ' ']).trim();
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = lower
            .strip_prefix("resume_action:")
            .or_else(|| lower.strip_prefix("next_action:"))
            .or_else(|| lower.strip_prefix("resume action:"))
            .or_else(|| lower.strip_prefix("next action:"))
        {
            return parse_compaction_resume_action_value(value);
        }
        if let Some(value) = trimmed
            .strip_prefix("下一步动作：")
            .or_else(|| trimmed.strip_prefix("下一步动作:"))
            .or_else(|| trimmed.strip_prefix("续跑动作："))
            .or_else(|| trimmed.strip_prefix("续跑动作:"))
        {
            return parse_compaction_resume_action_value(value);
        }
    }

    infer_compaction_resume_action_from_sections(&normalized)
}

pub(super) fn parse_compaction_resume_action_value(value: &str) -> CompactionResumeAction {
    let raw = value
        .trim()
        .trim_matches(['`', '"', '\'', '*', ' ', '-', ':', '：']);
    let direct = raw.trim();
    match direct {
        "最终回复" | "结束" | "完成" => return CompactionResumeAction::Final,
        "继续" => return CompactionResumeAction::Continue,
        "重试" | "修复" => return CompactionResumeAction::Retry,
        "询问用户" | "澄清" => return CompactionResumeAction::AskUser,
        _ => {}
    }
    let cleaned = raw.to_ascii_lowercase();
    let first = cleaned
        .split(|ch: char| ch.is_whitespace() || matches!(ch, ',' | ';' | '，' | '；'))
        .next()
        .unwrap_or_default()
        .trim();
    match first {
        "final" | "finish" | "done" | "complete" | "completed" | "answer" => {
            CompactionResumeAction::Final
        }
        "continue" | "next" | "work" | "run" => CompactionResumeAction::Continue,
        "retry" | "repair" | "fix" => CompactionResumeAction::Retry,
        "ask_user" | "ask-user" | "ask" | "clarify" | "clarification" => {
            CompactionResumeAction::AskUser
        }
        _ => CompactionResumeAction::Unknown,
    }
}

pub(super) fn infer_compaction_resume_action_from_sections(
    summary_text: &str,
) -> CompactionResumeAction {
    let body = summary_text.trim();
    if body.is_empty() {
        return CompactionResumeAction::Unknown;
    }
    if summary_has_failure_signal(body) {
        return CompactionResumeAction::Retry;
    }
    if summary_has_no_remaining_work_signal(body) {
        return CompactionResumeAction::Final;
    }
    if summary_has_pending_work_signal(body) {
        return CompactionResumeAction::Continue;
    }
    if summary_has_completed_signal(body) {
        return CompactionResumeAction::Final;
    }
    CompactionResumeAction::Unknown
}

pub(super) fn summary_has_completed_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("completed")
        || lower.contains("already produced")
        || lower.contains("already generated")
        || lower.contains("done")
        || text.contains("已完成")
        || text.contains("已经完成")
        || text.contains("已生成")
        || text.contains("已经生成")
        || text.contains("已绘制")
}

pub(super) fn summary_has_no_remaining_work_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("no remaining")
        || lower.contains("nothing remains")
        || lower.contains("no explicit todo")
        || lower.contains("no pending")
        || lower.contains("no further")
        || text.contains("无明确待办")
        || text.contains("没有明确待办")
        || text.contains("无需继续")
        || text.contains("没有剩余待办")
        || text.contains("无剩余待办")
}

pub(super) fn summary_has_pending_work_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("remaining todo")
        || lower.contains("pending")
        || lower.contains("next step")
        || lower.contains("todo:")
        || lower.contains("continue")
        || text.contains("待执行")
        || text.contains("剩余待办")
        || text.contains("下一步")
        || text.contains("继续")
}

pub(super) fn summary_has_failure_signal(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("failed")
        || lower.contains("failure")
        || lower.contains("error")
        || lower.contains("retry")
        || lower.contains("repair")
        || text.contains("失败")
        || text.contains("错误")
        || text.contains("修复")
        || text.contains("重试")
}

pub(super) fn build_current_turn_final_continuation_note(
    resume_action: CompactionResumeAction,
    summary_text: &str,
) -> String {
    let mut note = String::from("[Compaction continuation]\n\n[Compaction summary decision]\n");
    note.push_str("- resume_action: ");
    note.push_str(resume_action.as_str());
    append_compaction_summary_excerpt(&mut note, summary_text);
    note.push('\n');
    note.push_str(COMPACTION_CURRENT_TURN_FINAL_NOTE.trim());
    trim_text_to_tokens(
        &note,
        COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS.max(1_024),
        COMPACTION_TEXT_TRUNCATION_SUFFIX,
    )
}

pub(super) fn build_tool_success_continuation_note(
    progress: &CurrentTurnProgress,
    resume_action: CompactionResumeAction,
    summary_text: &str,
) -> String {
    let mut note = String::from("[Compaction continuation]");
    note.push_str("\n\n[Compaction summary decision]\n");
    note.push_str("- resume_action: ");
    note.push_str(resume_action.as_str());
    append_compaction_summary_excerpt(&mut note, summary_text);
    if let Some(snapshot) = progress.latest_tool_observation.as_ref() {
        note.push_str("\n\n[Latest retained tool observation]\n");
        note.push_str("- tool: ");
        note.push_str(snapshot.tool_name.trim());
        note.push('\n');
        if let Some(ok) = snapshot.ok {
            note.push_str("- ok: ");
            note.push_str(if ok { "true" } else { "false" });
            note.push('\n');
        }
        let summary = snapshot.summary.trim();
        if !summary.is_empty() {
            note.push_str("- summary: ");
            note.push_str(summary);
            note.push('\n');
        }
        if let Some(hint) = snapshot
            .next_step_hint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            note.push_str("- next_step_hint: ");
            note.push_str(hint);
            note.push('\n');
        }
    }
    note.push_str("\n");
    note.push_str(COMPACTION_CURRENT_TURN_SUCCESS_NOTE.trim());
    trim_text_to_tokens(
        &note,
        COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS.max(1_024),
        COMPACTION_TEXT_TRUNCATION_SUFFIX,
    )
}

pub(super) fn parse_compaction_observation_payload(content: &Value) -> Option<Value> {
    let raw = content.as_str()?.trim();
    let payload_text = raw
        .strip_prefix(OBSERVATION_PREFIX)
        .map(str::trim)
        .unwrap_or(raw);
    if payload_text.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(payload_text).ok()
}

pub(super) fn build_current_turn_replay_message(
    current_user_message: Option<&Value>,
    question_text: &str,
    message_budget: i64,
    progress: &CurrentTurnProgress,
    resume_action: CompactionResumeAction,
    summary_text: &str,
) -> CurrentUserReplay {
    match progress.state {
        CurrentTurnProgressState::Pending => {
            if let Some(message) = current_user_message {
                let mut trimmed = false;
                let mut candidate =
                    if let Some(next) = trim_message_to_fit_tokens(message, message_budget) {
                        trimmed = true;
                        next
                    } else {
                        message.clone()
                    };
                mark_current_user_message_inflight(&mut candidate);
                return CurrentUserReplay {
                    message: Some(candidate),
                    mode: CurrentUserReplayMode::Original,
                    trimmed,
                };
            }
            let question = question_text.trim();
            if question.is_empty() {
                return CurrentUserReplay {
                    message: None,
                    mode: CurrentUserReplayMode::Omitted,
                    trimmed: false,
                };
            }
            let mut placeholder = json!({ "role": "user", "content": question });
            let mut trimmed = false;
            if let Some(next) = trim_message_to_fit_tokens(&placeholder, message_budget) {
                placeholder = next;
                trimmed = true;
            }
            mark_current_user_message_inflight(&mut placeholder);
            CurrentUserReplay {
                message: Some(placeholder),
                mode: CurrentUserReplayMode::Placeholder,
                trimmed,
            }
        }
        CurrentTurnProgressState::ToolSucceeded => {
            let should_finalize = resume_action.should_finalize();
            let note = if should_finalize {
                build_current_turn_final_continuation_note(resume_action, summary_text)
            } else {
                build_tool_success_continuation_note(progress, resume_action, summary_text)
            };
            let mut message = json!({
                "role": "user",
                "content": note,
            });
            mark_current_user_message_inflight(&mut message);
            CurrentUserReplay {
                message: Some(message),
                mode: if should_finalize {
                    CurrentUserReplayMode::FinalContinuation
                } else {
                    CurrentUserReplayMode::ToolSuccessContinuation
                },
                trimmed: false,
            }
        }
        CurrentTurnProgressState::ToolFailed | CurrentTurnProgressState::InProgress => {
            let note = if progress.has_tool_failure {
                COMPACTION_CURRENT_TURN_REPAIR_NOTE
            } else if resume_action.should_finalize() {
                COMPACTION_CURRENT_TURN_FINAL_NOTE
            } else {
                COMPACTION_CURRENT_TURN_SUCCESS_NOTE
            };
            let mut message = json!({
                "role": "user",
                "content": note,
            });
            mark_current_user_message_inflight(&mut message);
            CurrentUserReplay {
                message: Some(message),
                mode: CurrentUserReplayMode::RepairContinuation,
                trimmed: false,
            }
        }
    }
}

pub(super) fn summarize_compaction_observation(content: &Value) -> String {
    let Some(payload) = parse_compaction_observation_payload(content) else {
        return extract_memory_summary_text_value(content);
    };
    summarize_compaction_observation_payload(&payload)
}

pub(super) fn summarize_compaction_observation_payload(payload: &Value) -> String {
    let Some(map) = payload.as_object() else {
        return extract_memory_summary_text_value(&payload);
    };
    let tool_name = map
        .get("tool")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let status = match map.get("ok").and_then(Value::as_bool) {
        Some(true) => "success",
        Some(false) => "failed",
        None => "recorded",
    };
    let mut headline = format!("Tool observation ({tool_name}): {status}");
    if let Some(code) = map
        .get("error_code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headline.push_str(&format!(" [{code}]"));
    }
    if let Some(compact_write) = summarize_compaction_write_file_observation(tool_name, map) {
        return compact_write;
    }
    let detail = extract_compaction_observation_detail(map);
    if detail.is_empty() {
        headline
    } else {
        format!("{headline}\n{detail}")
    }
}

pub(super) fn summarize_compaction_write_file_observation(
    tool_name: &str,
    payload: &Map<String, Value>,
) -> Option<String> {
    if !matches!(
        tool_name.trim(),
        "写入文件" | "write_file" | "文本编辑" | "edit_file2"
    ) {
        return None;
    }
    let data = payload.get("data")?.as_object()?;
    let path = data
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let existed = data
        .get("existed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let action = if existed {
        "Updated file"
    } else {
        "Created file"
    };
    Some(format!("{action} {path}"))
}

pub(super) fn extract_compaction_observation_detail(map: &Map<String, Value>) -> String {
    for key in ["error", "message", "summary", "preview"] {
        if let Some(value) = map.get(key) {
            if let Some(text) = extract_compaction_observation_text_candidate(value) {
                return text;
            }
        }
    }
    map.get("data")
        .and_then(extract_compaction_observation_text_candidate)
        .unwrap_or_default()
}

pub(super) fn extract_compaction_observation_text_candidate(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(_) | Value::Array(_) => {
            let text = extract_memory_summary_text_value(value);
            let cleaned = text.trim();
            if cleaned.is_empty() {
                None
            } else {
                Some(cleaned.to_string())
            }
        }
        Value::Object(map) => {
            for key in [
                "failure_summary",
                "error_detail_head",
                "summary",
                "preview",
                "result",
                "message",
                "stderr",
                "stdout",
                "content",
                "text",
                "structured_content",
            ] {
                if let Some(text) = map
                    .get(key)
                    .and_then(extract_compaction_observation_text_candidate)
                {
                    return Some(text);
                }
            }
            None
        }
        other => {
            let text = strip_tool_calls(&other.to_string()).trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
    }
}

pub(super) fn build_compaction_instruction(
    compaction_prompt: &str,
    artifact_content: &str,
    current_question: &str,
    current_user_signature: &str,
) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let prompt = compaction_prompt.trim();
    if !prompt.is_empty() {
        blocks.push(prompt.to_string());
    }
    let artifact = artifact_content.trim();
    if !artifact.is_empty() {
        blocks.push(artifact.to_string());
    }

    let mut request_candidates: Vec<String> = Vec::new();
    let question = current_question.trim();
    if !question.is_empty() {
        request_candidates.push(question.to_string());
    }
    let signature = current_user_signature.trim();
    if !signature.is_empty()
        && !request_candidates
            .iter()
            .any(|candidate| candidate == signature)
    {
        request_candidates.push(signature.to_string());
    }
    if !request_candidates.is_empty() {
        let request_block = request_candidates.join("\n");
        blocks.push(format!(
            "[Current user request / 当前用户问题]\n{request_block}\n\n[Compaction constraints / 压缩约束]\n- Treat the request above as explicit task context.\n- Do not write placeholder claims such as \"task unspecified\".\n- If evidence is missing, state \"Insufficient evidence in context\"."
        ));
    }

    blocks.join("\n\n")
}

pub(super) fn merge_compaction_system_message(
    system_message: Option<Value>,
    _artifact_content: &str,
) -> Option<Value> {
    match system_message {
        Some(mut message) => {
            if let Some(obj) = message.as_object_mut() {
                obj.insert("role".to_string(), Value::String("system".to_string()));
            }
            Some(message)
        }
        None => None,
    }
}

pub(super) fn build_compaction_summary_config(llm_config: &LlmModelConfig) -> LlmModelConfig {
    let mut summary_config = llm_config.clone();
    summary_config.max_rounds = Some(1);
    // Disable reasoning for compaction summaries to keep the auxiliary request lean.
    summary_config.reasoning_effort = Some(COMPACTION_SUMMARY_REASONING_EFFORT.to_string());
    summary_config
}

pub(super) fn strip_existing_memory_block_text(text: &str) -> String {
    let cleaned = text.trim_end();
    let mut prefixes = i18n::get_known_prefixes("memory.block_prefix");
    if prefixes.is_empty() {
        prefixes.push(i18n::t("memory.block_prefix"));
    }
    let mut cut_index: Option<usize> = None;
    for prefix in prefixes {
        let marker = prefix.trim();
        if marker.is_empty() {
            continue;
        }
        if let Some(index) = cleaned.find(marker) {
            cut_index = Some(cut_index.map_or(index, |current| current.min(index)));
        }
    }
    if let Some(index) = cut_index {
        cleaned[..index].trim_end().to_string()
    } else {
        cleaned.to_string()
    }
}

pub(super) fn merge_compaction_summary_with_fresh_memory(
    summary_text: &str,
    memory_block: &str,
) -> (String, bool) {
    let summary_without_memory = strip_existing_memory_block_text(summary_text);
    let memory_block = memory_block.trim();
    if memory_block.is_empty() {
        return (summary_without_memory, false);
    }
    let summary_without_memory = summary_without_memory.trim_end();
    if summary_without_memory.is_empty() {
        return (memory_block.to_string(), true);
    }
    (format!("{summary_without_memory}\n\n{memory_block}"), true)
}

pub(super) fn is_empty_compaction_summary(summary: &str) -> bool {
    let cleaned = summary.trim();
    if cleaned.is_empty() {
        return true;
    }
    let empty_summary = i18n::t("memory.empty_summary");
    if cleaned == empty_summary.trim() {
        return true;
    }
    let mut prefixes = i18n::get_known_prefixes("history.compaction_prefix");
    if prefixes.is_empty() {
        prefixes.push(i18n::t("history.compaction_prefix"));
    }
    for prefix in prefixes {
        if let Some(rest) = cleaned.strip_prefix(prefix.as_str()) {
            let rest = rest.trim();
            if rest.is_empty() || rest == empty_summary.trim() {
                return true;
            }
        }
    }
    false
}

pub(super) fn strip_compaction_prefix_text(summary: &str) -> String {
    let cleaned = summary.trim();
    if cleaned.is_empty() {
        return String::new();
    }
    for prefix in compaction_prefixes() {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            continue;
        }
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    cleaned.to_string()
}

pub(super) fn count_meaningful_chars(text: &str) -> usize {
    text.chars().filter(|ch| ch.is_alphanumeric()).count()
}

pub(super) fn trim_known_compaction_suffix(text: &str) -> &str {
    text.trim_end()
        .strip_suffix(COMPACTION_TEXT_TRUNCATION_SUFFIX)
        .map(str::trim_end)
        .unwrap_or_else(|| text.trim_end())
}

pub(super) fn is_placeholder_compaction_summary(summary: &str) -> bool {
    let body = strip_compaction_prefix_text(summary);
    let body = body.trim();
    if body.is_empty() {
        return true;
    }

    let compact: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return true;
    }

    let compact_ascii = compact.to_ascii_lowercase();
    if matches!(compact_ascii.as_str(), "..." | "...(" | "...(truncated)") {
        return true;
    }

    let meaningful = count_meaningful_chars(trim_known_compaction_suffix(body));
    meaningful < COMPACTION_MIN_SUMMARY_MEANINGFUL_CHARS
}

pub(super) fn is_invalid_compaction_summary(summary: &str) -> bool {
    is_empty_compaction_summary(summary) || is_placeholder_compaction_summary(summary)
}

pub(super) fn clamp_committed_compaction_summary(summary: &str, max_chars: usize) -> String {
    let body = strip_compaction_prefix_text(summary);
    let body = body.trim();
    if body.is_empty() || max_chars == 0 {
        return String::new();
    }
    let prefix = i18n::t("history.compaction_prefix");
    let reserved_chars = prefix.chars().count().saturating_add(1);
    let body_limit = max_chars.saturating_sub(reserved_chars).max(1);
    let clamped_body = trim_text_to_chars(body, body_limit, COMPACTION_TEXT_TRUNCATION_SUFFIX);
    HistoryManager::format_compaction_summary(&clamped_body)
}

pub(super) fn build_committable_compaction_summary(
    summary_candidate: &str,
    memory_block: &str,
) -> Option<(String, bool)> {
    if is_invalid_compaction_summary(summary_candidate) {
        return None;
    }
    let formatted = HistoryManager::format_compaction_summary(summary_candidate);
    let (merged, fresh_memory_injected) =
        merge_compaction_summary_with_fresh_memory(&formatted, memory_block);
    let committed = clamp_committed_compaction_summary(&merged, COMPACTION_SUMMARY_MAX_CHARS);
    if is_invalid_compaction_summary(&committed) {
        return None;
    }
    Some((committed, fresh_memory_injected))
}

pub(super) fn compaction_prefixes() -> Vec<String> {
    let mut prefixes = i18n::get_known_prefixes("history.compaction_prefix");
    if prefixes.is_empty() {
        prefixes.push(i18n::t("history.compaction_prefix"));
    }
    prefixes
}

pub(super) fn starts_with_compaction_prefix(text: &str) -> bool {
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return false;
    }
    compaction_prefixes()
        .iter()
        .map(|prefix| prefix.trim())
        .any(|prefix| !prefix.is_empty() && cleaned.starts_with(prefix))
}

pub(super) fn message_has_non_text_content(message: &Value) -> bool {
    let content = message.get("content").unwrap_or(&Value::Null);
    match content {
        Value::Array(parts) => parts.iter().any(|part| {
            let Some(obj) = part.as_object() else {
                return false;
            };
            let part_type = obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            part_type != "text" && (!part_type.is_empty() || obj.contains_key("image_url"))
        }),
        Value::Object(map) => {
            let part_type = map
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            part_type != "text" && (!part_type.is_empty() || map.contains_key("image_url"))
        }
        _ => false,
    }
}

pub(super) fn summarize_compaction_fallback_text(text: &str) -> String {
    let mut selected_lines = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let looks_like_metadata = line.starts_with('[') && line.contains(']');
        if looks_like_metadata {
            continue;
        }
        selected_lines.push(line);
        if selected_lines.len() >= 2 {
            break;
        }
    }

    let candidate = if selected_lines.is_empty() {
        text.trim().to_string()
    } else {
        selected_lines.join(" ")
    };
    let collapsed = candidate.split_whitespace().collect::<Vec<_>>().join(" ");
    trim_text_to_chars(&collapsed, 240, COMPACTION_TEXT_TRUNCATION_SUFFIX)
}

pub(super) fn build_compaction_fallback_summary(
    messages: &[Value],
    default_fallback: &str,
) -> String {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for message in messages.iter().rev() {
        let Some(obj) = message.as_object() else {
            continue;
        };
        let role = obj.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" || is_compaction_observation_message(role, obj) {
            continue;
        }
        let raw_text = extract_guard_content_text(obj.get("content").unwrap_or(&Value::Null));
        if raw_text.is_empty() || starts_with_compaction_prefix(&raw_text) {
            continue;
        }
        let text = summarize_compaction_fallback_text(&raw_text);
        if text.is_empty() || !seen.insert(text.clone()) {
            continue;
        }
        let label = if role == "assistant" {
            "Assistant"
        } else {
            "User"
        };
        entries.push(format!("- {label}: {text}"));
        if entries.len() >= 6 {
            break;
        }
    }

    entries.reverse();
    if entries.is_empty() {
        let fallback = summarize_compaction_fallback_text(default_fallback);
        if fallback.is_empty() {
            return i18n::t("compaction.summary_fallback");
        }
        return format!("Compressed earlier context.\n- User: {fallback}");
    }

    format!("Compressed earlier context.\n{}", entries.join("\n"))
}

#[cfg(test)]
pub(super) fn should_compact_by_context(
    context_tokens: i64,
    limit: i64,
    history_threshold: Option<i64>,
) -> (bool, bool) {
    let decision = super::compaction_policy::should_compact_by_context(
        context_tokens,
        limit,
        history_threshold,
    );
    (decision.by_history, decision.should_compact())
}

pub(super) fn resolve_message_budget(limit: i64) -> i64 {
    limit.max(1)
}

pub(super) fn resolve_projected_request_tokens(context_tokens: i64) -> i64 {
    context_tokens.max(0)
}

pub(super) fn resolve_compaction_limit(
    llm_config: &LlmModelConfig,
    context_tokens: i64,
    force: bool,
) -> Option<i64> {
    let configured_limit =
        HistoryManager::get_auto_compact_limit(llm_config).map(|limit| limit.max(1));
    if let Some(limit) = configured_limit {
        if force {
            return Some(resolve_force_compaction_limit(context_tokens, limit));
        }
        return Some(limit.max(1));
    }
    if !force {
        return None;
    }
    let adaptive_limit = (context_tokens / 4).max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    Some(adaptive_limit.clamp(
        COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS,
        COMPACTION_FORCE_FALLBACK_LIMIT,
    ))
}

pub(super) fn resolve_force_compaction_limit(context_tokens: i64, configured_limit: i64) -> i64 {
    if configured_limit <= 0 {
        return COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS.max(1);
    }
    if configured_limit <= COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS {
        return configured_limit.max(1);
    }
    let adaptive_limit =
        (context_tokens.saturating_mul(3) / 4).max(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    adaptive_limit
        .clamp(COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS, configured_limit)
        .max(1)
}

#[cfg(test)]
mod tests;
