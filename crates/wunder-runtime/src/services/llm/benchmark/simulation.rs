use super::{BenchmarkMetrics, ChatMessage, StreamStats};
use crate::services::virtual_llm::timing;
use serde_json::json;
use std::time::Instant;

pub(super) async fn simulate<F>(
    messages: &[ChatMessage],
    output_tokens: u32,
    speed: timing::VirtualModelSpeed,
    mut progress: F,
) -> Result<BenchmarkMetrics, String>
where
    F: FnMut(BenchmarkMetrics),
{
    let input = messages
        .iter()
        .map(|message| {
            crate::token_utils::estimate_message_tokens(
                &json!({ "role": message.role, "content": message.content }),
            )
        })
        .sum::<i64>()
        .max(0) as u64;
    let started = Instant::now();
    timing::wait_for_prefill(input, speed).await;
    let decode_start = tokio::time::Instant::now();
    let mut stats = StreamStats::default();
    let mut emitted = 0u32;
    // Reasoning is a subset of the requested output budget, never an extra generation.
    let reasoning_target = output_tokens / 4;
    let batch = (speed.generation_tokens_per_second() / 20).max(1) as u32;
    while emitted < output_tokens {
        let thinking = emitted < reasoning_target;
        let phase_end = if thinking {
            reasoning_target
        } else {
            output_tokens
        };
        // Never merge reasoning and answer deltas across a phase boundary.
        let count = if emitted == 0 {
            1
        } else {
            (phase_end - emitted).min(batch)
        };
        let next = emitted + count;
        tokio::time::sleep_until(decode_start + speed.generation_duration(u64::from(next - 1)))
            .await;
        let delta = if thinking {
            json!({"reasoning_content":" one".repeat(count as usize)})
        } else {
            json!({"content":" one".repeat(count as usize)})
        };
        let event = json!({"choices":[{"delta":delta}],"usage":{"prompt_tokens":input,"completion_tokens":next,"completion_tokens_details":{"reasoning_tokens":next.min(reasoning_target)}}});
        stats.event(&format!("data: {event}"), started.elapsed().as_secs_f64())?;
        progress(stats.metrics(output_tokens, started.elapsed().as_secs_f64(), false));
        emitted = next;
    }
    stats.event(
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}",
        started.elapsed().as_secs_f64(),
    )?;
    Ok(stats.metrics(output_tokens, started.elapsed().as_secs_f64(), true))
}
