use super::*;
use crate::services::runtime::thread::mailbox::MailboxGuard;

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn apply_agent_messages(
        &self,
        inbox: &MailboxGuard,
        user: &str,
        session: &str,
        messages: &mut Vec<Value>,
        emitter: &EventEmitter,
        round: RoundInfo,
        finishing: bool,
    ) -> Result<bool, OrchestratorError> {
        self.ensure_not_cancelled(session)?;
        let mut pending = inbox.take(finishing).into_iter();
        let mut applied = false;
        while let Some(message) = pending.next() {
            if message.cancelled() {
                emitter
                    .emit(
                        "subagent_message",
                        json!({"message_id":message.id,
                    "source_session_id":message.source,"session_id":session,"kind":message.kind,
                    "delivery":"not_applied"}),
                    )
                    .await;
                continue;
            }
            let mut data = json!({"message_id":message.id,"source_session_id":message.source,
                "session_id":session,"kind":message.kind,"message":message.text,"delivery":"applied"});
            round.insert_into(data.as_object_mut().expect("message payload"));
            let input = json!({"role":"user","content":format!("{OBSERVATION_PREFIX}{}", json!({
                "type":"subagent_message", "message_id":message.id,
                "source_session_id":message.source,"kind":message.kind,"message":message.text
            }))});
            // The receiving execution is the sole context writer. Never mutate system messages.
            if let Err(error) = self
                .workspace
                .append_model_context_entry(user, session, &input)
            {
                // Draining transfers ownership here; failures must settle every
                // accepted message, including the rest of this detached batch.
                for rejected in std::iter::once(message).chain(pending) {
                    emitter
                        .emit(
                            "subagent_message",
                            json!({"message_id":rejected.id,
                        "source_session_id":rejected.source,"session_id":session,
                        "kind":rejected.kind,"delivery":"not_applied"}),
                        )
                        .await;
                }
                return Err(OrchestratorError::internal(error.to_string()));
            }
            messages.push(input);
            emitter.emit("subagent_message", data).await;
            applied = true;
        }
        Ok(applied)
    }
}
