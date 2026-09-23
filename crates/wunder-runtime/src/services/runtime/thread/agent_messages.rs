use super::*;
use crate::services::runtime::thread::mailbox::AgentMessage;
use sha2::{Digest, Sha256};

fn message_task_id(user: &str, session: &str, source: &str, id: &str) -> String {
    let mut digest = Sha256::new();
    for part in [user, session, source, id] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    format!("agent_message_{:x}", digest.finalize())
}

impl ThreadRuntime {
    pub(super) async fn discard_agent_message(&self, task: &AgentTaskRecord) -> Result<()> {
        let storage = self.user_store.storage_backend();
        let id = task.task_id.clone();
        let retries = task.retry_count;
        let now = now_ts();
        blocking::run_db("thread.agent_message.discard", move || {
            storage.update_agent_task_status(UpdateAgentTaskStatusParams {
                task_id: &id,
                status: TASK_STATUS_CANCELLED,
                retry_count: retries,
                retry_at: now,
                started_at: None,
                finished_at: Some(now),
                last_error: Some("stale agent message"),
                updated_at: now,
            })
        })
        .await?;
        // Obsolete input is not a user cancellation. In particular, a completion
        // already collected by wait must not poison the parent or later reports.
        // Housekeeping must not let a slow stream/checkpoint block valid reports.
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.emit_queue_event(&task.session_id, &task.user_id, "subagent_message", json!({
                "queue_id":task.task_id,"session_id":task.session_id,"queue_status":"cancelled",
                "message_id":task.request_payload.pointer("/config_overrides/__agent_message/id"),
                "source_session_id":task.request_payload.pointer("/config_overrides/__agent_message/source"),
                "kind":task.request_payload.pointer("/config_overrides/__agent_message/kind"),
                "reason":"stale_agent_message","delivery":"not_applied"
            })),
        ).await;
        Ok(())
    }

    pub(crate) async fn existing_agent_message(
        &self,
        request: &WunderRequest,
        message: &AgentMessage,
    ) -> Result<Option<String>> {
        let id = message_task_id(
            &request.user_id,
            request.session_id.as_deref().unwrap_or_default(),
            &message.source,
            &message.id,
        );
        let storage = self.user_store.storage_backend();
        let question = request.question.clone();
        blocking::run_db("thread.agent_message.receipt", move || {
            let Some(task) = storage.get_agent_task(&id)? else {
                return Ok(None);
            };
            if task.request_payload.get("question").and_then(Value::as_str)
                != Some(question.as_str())
            {
                return Err(anyhow!("message_id conflict"));
            }
            Ok(Some(id))
        })
        .await
    }
    pub(super) async fn agent_message_is_current(&self, task: &AgentTaskRecord) -> Result<bool> {
        let Some(meta) = task
            .request_payload
            .pointer("/config_overrides/__agent_message")
        else {
            return Ok(true);
        };
        let source = meta
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let expected = meta
            .get("source_run_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        if meta.get("kind").and_then(Value::as_str) == Some("completion") {
            let run = meta.get("completion_run_id").and_then(Value::as_str);
            let dispatch = meta.get("dispatch_id").and_then(Value::as_str);
            if crate::services::subagents::is_auto_wake_consumed(&task.session_id, dispatch, run) {
                return Ok(false);
            }
        }
        if self.monitor.is_cancelled(&task.session_id) || self.monitor.is_cancelled(&source) {
            return Ok(false);
        }
        let storage = self.user_store.storage_backend();
        let user = task.user_id.clone();
        blocking::run_db("thread.agent_message.validate", move || {
            let Some(expected) = expected else {
                return Ok(true);
            };
            Ok(storage
                .list_session_runs_by_session(&user, &source, 1)?
                .into_iter()
                .next()
                .is_some_and(|run| {
                    run.run_id == expected
                        && !matches!(run.status.as_str(), "cancelled" | "timeout")
                }))
        })
        .await
    }

    /// Idle delivery uses the normal durable task ledger, lease and cancellation path.
    pub(crate) async fn submit_agent_message(
        self: &Arc<Self>,
        mut request: WunderRequest,
        message: &AgentMessage,
    ) -> Result<String> {
        message.validate()?;
        let session = request
            .session_id
            .as_deref()
            .ok_or_else(|| anyhow!("session required"))?
            .to_string();
        let task_id = message_task_id(&request.user_id, &session, &message.source, &message.id);
        if self.monitor.is_cancelled(&session) || message.cancelled() {
            return Err(anyhow!("parent or message source was interrupted"));
        }
        request.enforce_runtime_queue = true;
        request.client_message_id = Some(message.id.clone());
        let overrides = request.config_overrides.get_or_insert_with(|| json!({}));
        let completion = if message.kind == "completion" {
            request
                .question
                .strip_prefix(crate::orchestrator_constants::OBSERVATION_PREFIX)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
        } else {
            None
        };
        overrides["__agent_message"] = json!({"id":message.id,"source":message.source,
            "kind":message.kind,"completion_run_id": completion.as_ref().and_then(|value| value.pointer("/dispatch/run_id")),
            "dispatch_id":completion.as_ref().and_then(|value| value.pointer("/dispatch/dispatch_id"))});
        let mut payload = serde_json::to_value(&request)?;
        let store = self.user_store.storage_backend();
        let id = task_id.clone();
        let thread_id = format!("thread_{session}");
        let user = request.user_id.clone();
        let agent = request.agent_id.clone().unwrap_or_default();
        let source = message.source.clone();
        blocking::run_db("thread.agent_message.enqueue", move || {
            if let Some(existing) = store.get_agent_task(&id)? {
                if existing.request_payload.get("question") != payload.get("question") {
                    return Err(anyhow!("message_id conflict"));
                }
                return Ok(());
            }
            if !source.is_empty() {
                let run = store
                    .list_session_runs_by_session(&user, &source, 1)?
                    .into_iter()
                    .next();
                if let Some(run) = run {
                    if matches!(run.status.as_str(), "cancelled" | "timeout") {
                        return Err(anyhow!("message source was interrupted"));
                    }
                    payload["config_overrides"]["__agent_message"]["source_run_id"] =
                        json!(run.run_id);
                }
            }
            let now = now_ts();
            store.insert_agent_message_task(
                &AgentTaskRecord {
                    task_id: id,
                    thread_id,
                    user_id: user,
                    agent_id: agent,
                    session_id: session,
                    status: TASK_STATUS_PENDING.into(),
                    request_payload: payload,
                    request_id: None,
                    retry_count: 0,
                    retry_at: now,
                    created_at: now,
                    updated_at: now,
                    started_at: None,
                    finished_at: None,
                    last_error: None,
                },
                64,
            )
        })
        .await?;
        // A stop racing the DB write must not leave a new pending wake behind it.
        if self
            .monitor
            .is_cancelled(request.session_id.as_deref().unwrap_or_default())
            || message.cancelled()
        {
            self.cancel_task(&task_id)?;
            return Err(anyhow!("parent or message source was interrupted"));
        }
        self.clone().start();
        self.wake().await;
        Ok(task_id)
    }
}
