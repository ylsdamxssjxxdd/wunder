//! Prepare a provider response without executing tools or keeping per-session state.
use super::{
    capabilities::{self, VirtualModelError},
    VirtualReplayTurn,
};
use crate::{config::LlmModelConfig, schemas::TokenUsage, token_utils};
use serde_json::Value;

pub fn tool_tokens(calls: Option<&Value>) -> u64 {
    calls
        .filter(|value| !value.is_null())
        .map_or(0, |value| value.to_string().len().div_ceil(4) as u64)
}

pub fn prepare_turn(
    mut turn: VirtualReplayTurn,
    model: &LlmModelConfig,
    messages: &[Value],
    tools: Option<&[Value]>,
) -> Result<VirtualReplayTurn, VirtualModelError> {
    let output = model.max_output.unwrap_or(capabilities::DEFAULT_OUTPUT);
    let input = capabilities::validate_request(model, messages, tools, output)?;
    let has_tools = super::tool_protocol::adapt(&mut turn, model, tools)?;
    let reasoning_limit = capabilities::reasoning_budget(model, output);
    turn.reasoning =
        token_utils::trim_text_to_tokens(&turn.reasoning, i64::from(reasoning_limit), "");
    let reasoning = token_utils::approx_token_count(&turn.reasoning).max(0) as u64;
    let available = u64::from(output).saturating_sub(reasoning);
    let tools_cost = tool_tokens(turn.tool_calls.as_ref());
    let content_cost = token_utils::approx_token_count(&turn.content).max(0) as u64;
    let truncated = tools_cost.saturating_add(content_cost) > available;
    // Never hand partial JSON to the executor. A truncated tool call is not executable.
    if tools_cost > available || (has_tools && truncated && turn.tool_calls.is_none()) {
        turn.tool_calls = None;
        turn.content = " one".repeat(available.min(128) as usize);
    } else {
        turn.content =
            token_utils::trim_text_to_tokens(&turn.content, (available - tools_cost) as i64, "");
    }
    turn.finish_reason = Some(
        if truncated {
            "length"
        } else if turn.tool_calls.is_some() {
            "tool_calls"
        } else {
            "stop"
        }
        .into(),
    );
    let output = token_utils::approx_token_count(&turn.content).max(0) as u64
        + tool_tokens(turn.tool_calls.as_ref());
    // Recorded usage belongs to a different prompt; account for the current simulated request.
    turn.usage = Some(TokenUsage {
        input,
        output,
        total: input + output + reasoning,
        reasoning: Some(reasoning),
        estimated: true,
    });
    Ok(turn)
}

#[cfg(test)]
mod tests;
