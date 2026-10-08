//! Simulate reasoning then answer at the same token rate without retaining delta queues.
use super::{timing::VirtualModelSpeed, VirtualReplayTurn};
use anyhow::Result;
use tokio::time::{sleep_until, Instant};

pub async fn emit_virtual_deltas<F, Fut>(
    turn: &VirtualReplayTurn,
    stream: bool,
    speed: VirtualModelSpeed,
    mut on_delta: F,
) -> Result<()>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let started = Instant::now();
    let mut generated = 0;
    // Bound UI updates to roughly 20 Hz; accounting uses the shared UTF-8 estimator.
    let batch_bytes = (speed.generation_tokens_per_second() / 20).max(1) as usize * 4;
    for (text, reasoning) in [(&turn.reasoning, true), (&turn.content, false)] {
        let mut offset = 0;
        while offset < text.len() {
            let bytes = if generated == 0 { 4 } else { batch_bytes };
            let mut end = (offset + bytes).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let tokens = end.div_ceil(4) - offset.div_ceil(4);
            generated += tokens as u64;
            // Absolute deadlines prevent timer/callback overhead from accumulating per token.
            sleep_until(started + speed.generation_duration(generated.saturating_sub(1))).await;
            if stream {
                let delta = text[offset..end].to_string();
                if reasoning {
                    on_delta(String::new(), delta).await?;
                } else {
                    on_delta(delta, String::new()).await?;
                }
            }
            offset = end;
        }
    }
    // Tool argument generation costs time even though it is delivered atomically to the executor.
    generated += super::request::tool_tokens(turn.tool_calls.as_ref());
    sleep_until(started + speed.generation_duration(generated.saturating_sub(1))).await;
    Ok(())
}
