//! Budget allocation for retained context edges without changing replay order.
use super::memory_support::*;
use super::*;

pub(super) fn rebalance_retained_interaction_context(
    messages: &mut Vec<Value>,
    limit: i64,
    stats: &mut RebuiltContextGuardStats,
) {
    if messages.is_empty() || limit <= 0 {
        return;
    }
    let summary_index = locate_compaction_summary_message_index(messages);
    let current_index = locate_rebuilt_current_user_index(messages);
    let mut retained = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            is_retained_interaction_message(message)
                && Some(*index) != summary_index
                && Some(*index) != current_index
        })
        .map(|(index, message)| (index, estimate_message_tokens(message)))
        .collect::<Vec<_>>();
    if retained.is_empty() {
        return;
    }
    let retained_tokens = retained.iter().map(|(_, cost)| cost).sum::<i64>();
    let retained_floor = retained
        .iter()
        .map(|(_, cost)| (*cost).min(COMPACTION_MIN_RETAINED_INTERACTION_TOKENS))
        .sum::<i64>();
    let mut preserved_tokens = estimate_messages_tokens(messages) - retained_tokens;
    let mut excess = (preserved_tokens + retained_floor - limit).max(0);

    // Reserve readable excerpts before pruning the window. Otherwise a large
    // active request consumes the entire budget and permanently erases both edges.
    if excess > 0 {
        if let Some(index) = summary_index {
            let before = estimate_message_tokens(&messages[index]);
            let target = (before - excess)
                .max(COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS)
                .min(before);
            if let Some(trimmed) =
                trim_compaction_summary_message_to_fit_tokens(&messages[index], target)
            {
                let after = estimate_message_tokens(&trimmed);
                stats.summary_tokens_before = stats.summary_tokens_before.max(before);
                stats.summary_tokens_after = after;
                stats.summary_trimmed |= after < before;
                preserved_tokens -= before - after;
                excess = (excess - (before - after)).max(0);
                messages[index] = trimmed;
            }
        }
    }
    if excess > 0 {
        if let Some(index) = current_index.filter(|index| Some(*index) != summary_index) {
            let before = estimate_message_tokens(&messages[index]);
            let target = (before - excess)
                .max(COMPACTION_MIN_CURRENT_USER_MESSAGE_TOKENS)
                .min(before);
            if let Some(trimmed) = trim_message_to_fit_tokens(&messages[index], target) {
                let after = estimate_message_tokens(&trimmed);
                stats.current_user_tokens_before = stats.current_user_tokens_before.max(before);
                stats.current_user_tokens_after = after;
                stats.current_user_trimmed |= after < before;
                preserved_tokens -= before - after;
                messages[index] = trimmed;
            }
        }
    }

    let mut remaining = (limit - preserved_tokens).max(0);
    if retained_tokens <= remaining {
        return;
    }
    // Fund short replies first, then share the remaining budget between long
    // messages. A single long head message must not evict the next exchange.
    retained.sort_by_key(|(index, cost)| (*cost, *index));
    let mut removed = Vec::new();
    for (position, &(index, cost)) in retained.iter().enumerate() {
        let target = remaining / (retained.len() - position) as i64;
        if cost <= target {
            remaining -= cost;
            continue;
        }
        let candidate = trim_message_to_fit_tokens(&messages[index], target).filter(|message| {
            estimate_message_tokens(message) <= target
                && !extract_guard_content_text(&message["content"])
                    .trim()
                    .is_empty()
        });
        if let Some(candidate) = candidate {
            remaining -= estimate_message_tokens(&candidate);
            messages[index] = candidate;
        } else {
            removed.push(index);
        }
    }
    removed.sort_unstable();
    for index in removed.into_iter().rev() {
        messages.remove(index);
    }
}

#[cfg(test)]
mod tests;
