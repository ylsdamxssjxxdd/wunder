use super::{EventEmitter, Orchestrator, OrchestratorError, RoundInfo};
use crate::config::LlmModelConfig;
use crate::services::llm::output::OutputDiagnostics;
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct EmptyOutputGuard {
    recovery_used: bool,
    /// The recovery round must go out with thinking off. This is the recovery
    /// retry itself, not a property of the whole turn: latching it until the
    /// turn ended meant one exhausted thinking budget silenced the model's
    /// thoughts for every later round of a long tool-driven turn. The flag is
    /// cleared once that round is answered, so a request the round re-issues
    /// (e.g. after context-overflow compaction) still runs with thinking off.
    thinking_override: bool,
    last_output: OutputDiagnostics,
}

impl EmptyOutputGuard {
    pub(super) fn observe(&mut self, output: OutputDiagnostics) {
        // The streaming client can spend the turn's single recovery itself by
        // resending with thinking disabled when a thinking budget is exhausted.
        // Count that resend here; otherwise the orchestrator would grant a
        // second recovery and send another pair of requests for one empty
        // answer. The budget is deliberately turn-wide, not per layer.
        if output.transport_recovery_used {
            self.recovery_used = true;
        }
        // The recovery round has been answered, so the user's reasoning setting
        // applies again from the next round on. (A transport recovery already
        // ran without thinking inside its own call.) Clearing on the answer
        // rather than when the override is read keeps it in force for every
        // attempt that round re-issues.
        self.thinking_override = false;
        self.last_output = output;
    }

    /// Answers "does the next request need thinking off", never "is this turn
    /// in recovery mode". The flag stays set until the round is observed.
    pub(super) fn config_override(&self, config: &LlmModelConfig) -> Option<LlmModelConfig> {
        if !self.thinking_override {
            return None;
        }
        let mut recovery = config.clone();
        recovery.reasoning_effort = Some("none".into());
        recovery.thinking_token_budget = None;
        Some(recovery)
    }

    pub(super) fn exhausted_error(&self) -> OrchestratorError {
        let mut detail = json!({
            "reason": "unusable_output_loop",
            "recovery_attempts": u32::from(self.recovery_used),
            "max_recovery_attempts": 1,
            "thinking_disabled_for_recovery": self.recovery_used,
        });
        self.last_output.insert_into(&mut detail);
        OrchestratorError::new(
            "LLM_OUTPUT_LOOP",
            "Model produced no usable answer or tool call; automatic output recovery stopped. Retry the turn or adjust the model configuration.".into(),
            Some(detail),
        )
    }

    fn recover(
        &mut self,
        allow_tools: bool,
        reason: &'static str,
    ) -> Result<Value, OrchestratorError> {
        if self.recovery_used {
            return Err(self.exhausted_error());
        }
        // This budget lasts for the entire user turn, even after successful tool calls.
        // Alternating empty answers and valid calls must not replenish recovery attempts.
        self.recovery_used = true;
        // Only the retry this notice triggers runs without thinking.
        self.thinking_override = true;
        let instruction = if allow_tools {
            "Stop reasoning. Output one allowed tool call with complete JSON arguments, or a concise final answer now. Do not repeat task analysis. Split large tool payloads into smaller complete calls."
        } else {
            "Stop reasoning. Respond directly with a concise final answer now. Do not repeat task analysis."
        };
        let mut notice = json!({
            "type": "empty_final_answer_notice", "ok": false, "reason": reason,
            "attempt": 1, "max_attempts": 1, "instruction": instruction,
        });
        self.last_output.insert_into(&mut notice);
        Ok(notice)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn recover_or_stop(
        &mut self,
        orchestrator: &Orchestrator,
        messages: &mut Vec<Value>,
        user_id: &str,
        session_id: &str,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        allow_tools: bool,
        reason: &'static str,
    ) -> Result<(), OrchestratorError> {
        let notice = self.recover(allow_tools, reason)?;
        let mut progress = notice.clone();
        progress["stage"] = json!("empty_final_answer_reroute");
        progress["recovery_action"] = json!("disable_thinking");
        round_info.insert_into(progress.as_object_mut().expect("output recovery object"));
        emitter.emit("progress", progress).await;
        let message = json!({
            "role": "user",
            "content": super::execute_support::encode_observation_prefixed_json(&notice),
        });
        orchestrator
            .try_append_internal_model_context_chat(
                user_id,
                session_id,
                &message,
                "empty_output_recovery",
                round_info,
            )
            .await
            .map_err(|error| OrchestratorError::internal(error.to_string()))?;
        messages.push(message);
        Ok(())
    }
}

// Only the fallback wrapper for unparsed JSON is rejected. Ordinary raw text and
// structured objects with a legitimate raw field keep their existing semantics.
pub(super) fn has_incomplete_arguments(arguments: &Value) -> bool {
    let raw = match arguments {
        Value::String(raw) => Some(raw.as_str()),
        Value::Object(map) if map.len() == 1 => map.get("raw").and_then(Value::as_str),
        _ => None,
    };
    raw.map(str::trim).is_some_and(|raw| {
        (raw.starts_with('{') || raw.starts_with('['))
            && serde_json::from_str::<Value>(raw).is_err_and(|error| error.is_eof())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_disables_thinking_once_without_mutating_original_config() {
        let config = LlmModelConfig {
            thinking_token_budget: Some(4096),
            ..Default::default()
        };
        let mut guard = EmptyOutputGuard::default();
        assert!(guard.config_override(&config).is_none());
        guard.recover(true, "empty_output").unwrap();
        let recovery = guard.config_override(&config).unwrap();
        assert_eq!(
            (
                recovery.reasoning_effort.as_deref(),
                recovery.thinking_token_budget
            ),
            (Some("none"), None)
        );
        assert_eq!(config.thinking_token_budget, Some(4096));
        guard.observe(OutputDiagnostics::default());
        let error = guard.recover(false, "empty_output").unwrap_err();
        assert_eq!(
            (error.code(), error.recovery_action()),
            ("LLM_OUTPUT_LOOP", "retry_next_turn")
        );
    }

    /// One exhausted thinking budget used to latch `thinking: none` on every
    /// later round of the turn, so a 31-round tool run only ever thought in the
    /// first few rounds. The override belongs to the recovery round: every
    /// attempt it re-issues keeps thinking off, and the answer releases it.
    #[test]
    fn thinking_override_covers_only_the_recovery_round() {
        let config = LlmModelConfig {
            thinking_token_budget: Some(4096),
            ..Default::default()
        };
        let mut guard = EmptyOutputGuard::default();
        guard.recover(true, "empty_output").unwrap();
        assert!(
            guard.config_override(&config).is_some(),
            "the retry the recovery notice triggers runs without thinking"
        );
        assert!(
            guard.config_override(&config).is_some(),
            "a re-issued attempt of the recovery round still runs without thinking"
        );
        guard.observe(OutputDiagnostics::default());
        assert!(
            guard.config_override(&config).is_none(),
            "the round after the answered recovery gets the user's reasoning setting back"
        );
        assert!(
            guard.recover(true, "empty_output").is_err(),
            "the recovery budget itself stays turn-wide"
        );
    }

    /// The transport resend already disabled thinking inside its own call, so it
    /// must not ask for a second override on the following round.
    #[test]
    fn transport_recovery_spends_the_budget_without_latching_the_override() {
        let config = LlmModelConfig {
            thinking_token_budget: Some(4096),
            ..Default::default()
        };
        let mut guard = EmptyOutputGuard::default();
        guard.observe(OutputDiagnostics {
            transport_recovery_used: true,
            ..Default::default()
        });
        assert!(
            guard.config_override(&config).is_none(),
            "the next round thinks again"
        );
        assert!(
            guard.recover(true, "empty_output").is_err(),
            "a second recovery in the same turn is still refused"
        );
    }

    #[test]
    fn incomplete_json_is_rejected_without_rejecting_raw_text() {
        for value in [json!({"raw":"{\"path\":\""}), json!("[1,")] {
            assert!(has_incomplete_arguments(&value));
        }
        for value in [
            json!({"raw":"text"}),
            json!({"raw":"{}"}),
            json!({"path":"file", "raw":"{"}),
            json!({"path":"file"}),
        ] {
            assert!(!has_incomplete_arguments(&value));
        }
    }
}
