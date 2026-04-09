// 调度常量：统一历史压缩、SSE 与会话锁的关键参数。
pub const OBSERVATION_PREFIX: &str = "tool_response: ";
pub const COMPACTION_META_TYPE: &str = "compaction_summary";
pub const COMPACTION_RATIO: f64 = 0.9;
pub const COMPACTION_HISTORY_RATIO: f64 = 0.9;
pub const COMPACTION_OUTPUT_RESERVE: i64 = 1024;
pub const COMPACTION_SAFETY_MARGIN: i64 = 512;
pub const COMPACTION_SUMMARY_MAX_OUTPUT: i64 = 1024;
pub const COMPACTION_MIN_OBSERVATION_TOKENS: i64 = 128;
pub const COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS: i64 = 2048;
pub const COMPACTION_FORCE_FALLBACK_LIMIT: i64 = 8192;
pub const ARTIFACT_INDEX_MAX_ITEMS: i64 = 200;
pub const SESSION_LOCK_TTL_S: f64 = 120.0;
pub const SESSION_LOCK_HEARTBEAT_S: f64 = 5.0;
pub const SESSION_LOCK_POLL_INTERVAL_S: f64 = 0.2;
pub const SESSION_LOCK_BUSY_RETRY_S: f64 = 0.8;
pub const STREAM_EVENT_QUEUE_SIZE: usize = 256;
pub const STREAM_EVENT_POLL_INTERVAL_S: f64 = 0.2;
pub const STREAM_EVENT_RESUME_POLL_INTERVAL_S: f64 = 0.08;
pub const STREAM_EVENT_RESUME_POLL_MAX_INTERVAL_S: f64 = 0.8;
pub const STREAM_EVENT_RESUME_POLL_BACKOFF_FACTOR: f64 = 1.6;
pub const STREAM_EVENT_RESUME_POLL_BACKOFF_AFTER: usize = 3;
pub const STREAM_EVENT_SLOW_CLIENT_QUEUE_WATERMARK: usize = 2;
pub const STREAM_EVENT_SLOW_CLIENT_WARN_INTERVAL_S: u64 = 3;
pub const STREAM_EVENT_FETCH_LIMIT: i64 = 200;
pub const STREAM_EVENT_TTL_S: f64 = 3600.0;
pub const STREAM_EVENT_CLEANUP_INTERVAL_S: f64 = 60.0;
pub const STREAM_EVENT_PERSIST_INTERVAL_MS: u64 = 80;
pub const STREAM_EVENT_PERSIST_CHARS: usize = 160;
pub const DEFAULT_LLM_TIMEOUT_S: u64 = 120;
pub const DEFAULT_TOOL_TIMEOUT_S: f64 = 120.0;
pub const MIN_TOOL_TIMEOUT_S: f64 = 1.0;

pub const DEFAULT_TOOL_PARALLELISM: usize = 4;
pub const MAX_USER_INPUT_TEXT_CHARS: usize = 1 << 20;
pub const TOOL_RESULT_HEAD_CHARS: usize = 10000;
pub const TOOL_RESULT_TAIL_CHARS: usize = 10000;
pub const TOOL_RESULT_MAX_CHARS: usize = TOOL_RESULT_HEAD_CHARS + TOOL_RESULT_TAIL_CHARS;
pub const TOOL_RESULT_TRUNCATION_MARKER: &str = "...(truncated)...";
pub const TOOL_RESULT_MAX_ARRAY_ITEMS: usize = 128;
pub const TOOL_RESULT_ARRAY_HEAD_ITEMS: usize = 48;
pub const TOOL_RESULT_ARRAY_TAIL_ITEMS: usize = 16;
pub const TOOL_RESULT_PAGINATED_MAX_ARRAY_ITEMS: usize = 500;
pub const TOOL_RESULT_PAGINATED_ARRAY_HEAD_ITEMS: usize = 180;
pub const TOOL_RESULT_PAGINATED_ARRAY_TAIL_ITEMS: usize = 60;

pub fn truncate_tool_result_text(value: &str) -> String {
    truncate_tool_result_text_with_budget(value, TOOL_RESULT_MAX_CHARS)
}

pub fn truncate_tool_result_text_with_budget(value: &str, budget_chars: usize) -> String {
    let value_len = value.chars().count();
    if value_len <= budget_chars {
        return value.to_string();
    }
    let marker_chars = TOOL_RESULT_TRUNCATION_MARKER.chars().count();
    if budget_chars <= marker_chars {
        return TOOL_RESULT_TRUNCATION_MARKER
            .chars()
            .take(budget_chars)
            .collect();
    }
    let visible_chars = budget_chars.saturating_sub(marker_chars);
    let head_chars = visible_chars / 2;
    let tail_chars = visible_chars.saturating_sub(head_chars);
    truncate_tool_result_head_tail(value, head_chars, tail_chars, TOOL_RESULT_TRUNCATION_MARKER)
}

fn truncate_tool_result_head_tail(
    value: &str,
    head_chars: usize,
    tail_chars: usize,
    marker: &str,
) -> String {
    let value_len = value.chars().count();
    if value_len <= head_chars + tail_chars {
        return value.to_string();
    }
    let head_chars = head_chars.min(value_len);
    let tail_chars = tail_chars.min(value_len.saturating_sub(head_chars));
    let mut output = String::new();
    output.extend(value.chars().take(head_chars));
    output.push_str(marker);
    if tail_chars > 0 {
        output.extend(value.chars().skip(value_len - tail_chars).take(tail_chars));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{truncate_tool_result_text, truncate_tool_result_text_with_budget};
    use crate::orchestrator::{
        TOOL_RESULT_HEAD_CHARS, TOOL_RESULT_MAX_CHARS, TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    };

    #[test]
    fn truncate_tool_result_text_keeps_head_and_tail() {
        let input = format!(
            "{}{}",
            "a".repeat(TOOL_RESULT_HEAD_CHARS + 64),
            "z".repeat(TOOL_RESULT_TAIL_CHARS + 64)
        );
        let output = truncate_tool_result_text(&input);
        let marker_chars = TOOL_RESULT_TRUNCATION_MARKER.chars().count();
        let visible_chars = TOOL_RESULT_MAX_CHARS - marker_chars;
        let expected_head_chars = visible_chars / 2;
        let expected_tail_chars = visible_chars - expected_head_chars;

        assert!(output.starts_with(&"a".repeat(expected_head_chars)));
        assert!(output.contains(TOOL_RESULT_TRUNCATION_MARKER));
        assert!(output.ends_with(&"z".repeat(expected_tail_chars.min(TOOL_RESULT_TAIL_CHARS))));
    }

    #[test]
    fn truncate_tool_result_text_with_small_budget_returns_marker_prefix() {
        let output = truncate_tool_result_text_with_budget("abcdefghijklmnopqrstuvwxyz", 5);

        assert_eq!(output, "...(t".to_string());
    }
}
