//! Identical synthetic request facts for local and service canonical writers.

use super::{
    synthetic_system_prompt_tokens, RoundPlan, StressThreadScenario, SYNTHETIC_PROFILE_ID,
};
use serde_json::{json, Value};
use wunder_core::{
    llm_speed::{LlmSpeedSummary, TurnDecodeSpeedAccumulator},
    thread_store::{TurnId, UsageRecord},
};

#[derive(Default)]
pub(crate) struct StressUsageAccumulator {
    input: u64,
    output: u64,
    reasoning: u64,
}

impl StressUsageAccumulator {
    fn record(&mut self, plan: &RoundPlan) {
        self.input = self.input.saturating_add(plan.input_tokens);
        self.output = self.output.saturating_add(plan.answer_tokens);
        self.reasoning = self
            .reasoning
            .saturating_add(plan.reasoning_tokens.unwrap_or_default());
    }

    fn value(&self) -> Value {
        json!({
            "input_tokens": self.input,
            "output_tokens": self.output,
            "reasoning_tokens": self.reasoning,
            "total_tokens": self.input.saturating_add(self.output).saturating_add(self.reasoning),
            "estimated": true,
        })
    }
}

#[derive(Default)]
pub(crate) struct StressTurnStatistics {
    usage: StressUsageAccumulator,
    speed: TurnDecodeSpeedAccumulator,
    interaction_duration_s: f64,
    model_requests: u64,
    completed_tools: u64,
}

pub(crate) struct StressRequestStatistics {
    pub request_token_breakdown: Value,
    pub message_stats: Value,
    pub usage: UsageRecord,
}

impl StressTurnStatistics {
    /// Snapshot the same boundary as a live assistant item: the provider has
    /// responded, but its newly proposed tools have not run yet. These are
    /// synthetic normal-user credits, never debits to a real account.
    pub(crate) fn record_model(
        &mut self,
        thread_usage: &mut StressUsageAccumulator,
        seed: u64,
        scenario: StressThreadScenario,
        prompt: &str,
        turn: &TurnId,
        plan: &RoundPlan,
    ) -> StressRequestStatistics {
        let breakdown = request_token_breakdown(seed, scenario, prompt, self.model_requests, plan);
        self.model_requests = self.model_requests.saturating_add(1);
        self.usage.record(plan);
        thread_usage.record(plan);
        self.interaction_duration_s += plan.prefill_duration_s + plan.decode_duration_s;
        let decode_tokens = plan
            .answer_tokens
            .saturating_add(plan.reasoning_tokens.unwrap_or_default());
        let speed = LlmSpeedSummary::from_usage_and_durations(
            Some(plan.input_tokens),
            Some(decode_tokens),
            Some(plan.prefill_duration_s),
            Some(plan.decode_duration_s),
        );
        self.speed.record_summary(&speed);
        let mut response_usage = StressUsageAccumulator::default();
        response_usage.record(plan);
        let response_usage = response_usage.value();
        let turn_usage = self.usage.value();
        let mut message_stats = json!({
            "usage": response_usage,
            "round_usage": turn_usage,
            "interaction_duration_s": self.interaction_duration_s,
            "model_request_count": self.model_requests,
            "account_credits_consumed": self.model_requests,
            "toolCalls": self.completed_tools,
            "tool_calls": self.completed_tools,
            "contextTokens": plan.input_tokens,
            "context_occupancy_tokens": plan.input_tokens,
            "model_duration_s": plan.prefill_duration_s + plan.decode_duration_s,
            "profile": SYNTHETIC_PROFILE_ID,
            "scenario": scenario.as_str(),
        });
        let map = message_stats.as_object_mut().expect("statistics object");
        speed.insert_into_map(map);
        self.speed.insert_into_map(map);
        StressRequestStatistics {
            request_token_breakdown: breakdown.clone(),
            message_stats,
            usage: UsageRecord {
                turn_id: Some(turn.clone()),
                input_tokens: plan.input_tokens,
                output_tokens: plan.answer_tokens,
                metadata: json!({
                    "total_tokens": response_usage["total_tokens"],
                    "reasoning_tokens": plan.reasoning_tokens.unwrap_or_default(),
                    "estimated": true,
                    "model": "stress-model",
                    "model_round": self.model_requests,
                    "response_id": format!("{turn}:response-{}", self.model_requests - 1),
                    "profile": SYNTHETIC_PROFILE_ID,
                    "scenario": scenario.as_str(),
                    "turn_usage": turn_usage,
                    "thread_usage": thread_usage.value(),
                    "request_token_breakdown": breakdown,
                }),
            },
        }
    }

    pub(crate) fn complete_tool(&mut self, duration_s: f64) {
        self.completed_tools = self.completed_tools.saturating_add(1);
        self.interaction_duration_s += duration_s;
    }
}

fn request_token_breakdown(
    seed: u64,
    scenario: StressThreadScenario,
    prompt: &str,
    model_index: u64,
    plan: &RoundPlan,
) -> Value {
    let system = synthetic_system_prompt_tokens(seed, scenario);
    let latest_user = (prompt.len() as u64).div_ceil(4).max(1);
    let tool_calls = (plan.tool_count as u64).saturating_mul(24);
    let tool_ids = (plan.tool_count as u64).saturating_mul(8);
    let envelope = 8_u64;
    let protocol = 16_u64;
    let accounted = system
        .saturating_add(latest_user)
        .saturating_add(tool_calls)
        .saturating_add(tool_ids)
        .saturating_add(envelope)
        .saturating_add(protocol);
    // The sampler supplies a deliberately substantial history budget. Output
    // reasoning is excluded from request occupancy, as in production.
    let estimated_input = plan.input_tokens.max(accounted);
    json!({
        "native_tool_tokens": 0,
        "system_prompt_tokens": system,
        "history_tokens": estimated_input.saturating_sub(accounted),
        "latest_user_tokens": latest_user,
        "image_tokens": 0,
        "reasoning_tokens": 0,
        "tool_call_tokens": tool_calls,
        "tool_call_id_tokens": tool_ids,
        "message_envelope_tokens": envelope,
        "fixed_protocol_tokens": protocol,
        "message_tokens": estimated_input,
        "estimated_input_tokens": estimated_input,
        "tool_count": plan.tool_count,
        "message_count": model_index.saturating_add(2),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::stress_model::iter_turn_plans_for_scenario;

    #[test]
    fn shared_statistics_preserve_breakdown_and_cumulative_request_facts() {
        for scenario in [
            StressThreadScenario::Typical,
            StressThreadScenario::Complex,
            StressThreadScenario::Adversarial,
        ] {
            let mut thread_usage = StressUsageAccumulator::default();
            let mut total_input = 0_u64;
            for number in 1..=2 {
                let mut turn_stats = StressTurnStatistics::default();
                let turn = TurnId::new(format!("sample-{number}")).unwrap();
                let mut duration = 0.0;
                for (index, plan) in
                    iter_turn_plans_for_scenario(7, number, 4, scenario).enumerate()
                {
                    let stats = turn_stats.record_model(
                        &mut thread_usage,
                        7,
                        scenario,
                        "sample input",
                        &turn,
                        &plan,
                    );
                    let breakdown = &stats.request_token_breakdown;
                    assert_eq!(breakdown.as_object().unwrap().len(), 14);
                    let components = [
                        "native_tool_tokens",
                        "system_prompt_tokens",
                        "history_tokens",
                        "latest_user_tokens",
                        "image_tokens",
                        "reasoning_tokens",
                        "tool_call_tokens",
                        "tool_call_id_tokens",
                        "message_envelope_tokens",
                        "fixed_protocol_tokens",
                    ];
                    assert_eq!(
                        components
                            .into_iter()
                            .map(|k| breakdown[k].as_u64().unwrap())
                            .sum::<u64>(),
                        plan.input_tokens
                    );
                    total_input += plan.input_tokens;
                    assert_eq!(
                        stats.usage.metadata["thread_usage"]["input_tokens"],
                        total_input
                    );
                    assert_eq!(stats.message_stats["account_credits_consumed"], index + 1);
                    assert_eq!(
                        stats.message_stats["avg_model_round_speed_rounds"],
                        index + 1
                    );
                    assert_eq!(
                        stats.message_stats["usage"]["input_tokens"],
                        breakdown["estimated_input_tokens"]
                    );
                    duration += plan.prefill_duration_s + plan.decode_duration_s;
                    assert!(
                        (stats.message_stats["interaction_duration_s"]
                            .as_f64()
                            .unwrap()
                            - duration)
                            .abs()
                            < 1e-9
                    );
                    for seconds in plan.tool_duration_s {
                        turn_stats.complete_tool(seconds);
                        duration += seconds;
                    }
                }
            }
        }
    }
}
