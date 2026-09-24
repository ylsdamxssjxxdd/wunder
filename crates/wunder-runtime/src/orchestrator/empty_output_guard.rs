use super::{EventEmitter, Orchestrator, OrchestratorError, RoundInfo};
use crate::config::LlmModelConfig;
use crate::services::llm::output::OutputDiagnostics;
use serde_json::{json, Value};

#[derive(Default)]
pub(super) struct EmptyOutputGuard {
    recovery_used: bool,
    last_output: OutputDiagnostics,
}

impl EmptyOutputGuard {
    pub(super) fn observe(&mut self, output: OutputDiagnostics) {
        self.last_output = output;
    }

    pub(super) fn config_override(&self, config: &LlmModelConfig) -> Option<LlmModelConfig> {
        self.recovery_used.then(|| {
            let mut recovery = config.clone();
            recovery.reasoning_effort = Some("none".into());
            recovery.thinking_token_budget = None;
            recovery
        })
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
        orchestrator.append_model_context_entry(user_id, session_id, &message);
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
