use super::{EventEmitter, Orchestrator, OrchestratorError, RoundInfo, TokenUsage};
use serde_json::json;

pub(super) fn request_context_tokens(usage: &TokenUsage) -> Option<i64> {
    (!usage.estimated && usage.input > 0).then_some(usage.input.min(i64::MAX as u64) as i64)
}

impl Orchestrator {
    // Account completed provider responses before validating their tool calls.
    // Invalid calls still consumed tokens; network failures without usage do not.
    pub(super) async fn account_model_usage(
        &self,
        usage: &TokenUsage,
        emitter: &EventEmitter,
        user_id: &str,
        is_admin: bool,
        round: RoundInfo,
        emit_events: bool,
        purpose: &str,
    ) -> Result<(), OrchestratorError> {
        let cumulative = emitter.record_usage(usage);
        let mut payload = json!({
            "usage": usage,
            "round_usage": cumulative,
            "request_consumed_tokens": cumulative.total,
            "purpose": purpose,
        });
        round.insert_into(payload.as_object_mut().expect("usage object"));
        // Cumulative snapshots are replay-safe, including partial failed turns.
        emitter.emit("model_usage", payload).await;
        if !is_admin {
            self.consume_user_tokens(
                user_id,
                usage.total.min(i64::MAX as u64) as i64,
                emitter,
                round,
                emit_events,
            )
            .await?;
        }
        Ok(())
    }
}

pub(super) fn accumulate_usage(target: &mut TokenUsage, usage: &TokenUsage) {
    target.input = target.input.saturating_add(usage.input);
    target.output = target.output.saturating_add(usage.output);
    target.total = target.total.saturating_add(usage.total);
    // A mixed turn has no exact reasoning split if any response omitted it.
    target.reasoning = match (target.reasoning, usage.reasoning) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        _ => None,
    };
    target.estimated |= usage.estimated;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_context_uses_only_observed_input() {
        let mut usage = TokenUsage {
            input: 120,
            output: 10,
            total: 170,
            reasoning: Some(40),
            estimated: false,
        };
        assert_eq!(request_context_tokens(&usage), Some(120));
        usage.estimated = true;
        assert_eq!(request_context_tokens(&usage), None);
        usage.estimated = false;
        usage.input = 0;
        assert_eq!(request_context_tokens(&usage), None);
    }

    #[test]
    fn includes_rejected_and_summary_responses_without_fabricating_unknown_reasoning() {
        let mut total = TokenUsage {
            reasoning: Some(0),
            ..Default::default()
        };
        for usage in [
            TokenUsage {
                input: 100,
                output: 10,
                total: 130,
                reasoning: Some(20),
                estimated: false,
            },
            TokenUsage {
                input: 120,
                output: 20,
                total: 140,
                reasoning: None,
                estimated: false,
            },
            TokenUsage {
                input: 30,
                output: 10,
                total: 40,
                reasoning: Some(0),
                estimated: true,
            },
        ] {
            accumulate_usage(&mut total, &usage);
        }
        assert_eq!(
            total,
            TokenUsage {
                input: 250,
                output: 40,
                total: 310,
                reasoning: None,
                estimated: true
            }
        );
    }
}
