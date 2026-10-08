use super::*;

impl ThreadRuntime {
    pub async fn prioritize_session(&self, session_id: &str, authorized: bool) -> Result<Value> {
        if !authorized {
            return Err(anyhow!(i18n::t("error.permission_denied")));
        }
        if !self.config_store.get().await.agent_queue.enabled {
            return Err(anyhow!("queue is disabled"));
        }
        let store = self.user_store.clone();
        let thread_id = format!("thread_{}", session_id.trim());
        let task = blocking::run_db("queue.admin_promote", move || {
            let mut tasks = store.list_agent_tasks_by_thread(&thread_id, Some("pending"), 256)?;
            tasks.extend(store.list_agent_tasks_by_thread(&thread_id, Some("retry"), 256)?);
            tasks.sort_by(|a, b| {
                a.created_at
                    .total_cmp(&b.created_at)
                    .then(a.task_id.cmp(&b.task_id))
            });
            let task = tasks
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no pending task"))?;
            if !store
                .storage_backend()
                .promote_agent_task(&task.task_id, now_ts())?
            {
                return Err(anyhow!("task is no longer pending"));
            }
            Ok(task)
        })
        .await?;
        let config = self.config_store.get().await;
        let paused_session = if self.active_runtime_session_count()
            >= config.server.max_active_sessions.max(1)
            && !self.session_has_active_runtime_slot(&task.session_id)
        {
            self.orchestrator.scheduling.request_slot(&task.session_id)
        } else {
            None
        };
        // The active emitter owns that session's event sequence. It publishes the handoff
        // at the safe boundary; do not append a competing queue event to the victim stream.
        if !self.session_has_active_runtime_slot(&task.session_id) {
            self.emit_queue_event(&task.session_id, &task.user_id, "queue_update", json!({
            "queue_id":task.task_id, "session_id":task.session_id, "queue_priority":1,
            "reason":"admin_priority", "client_message_id":task.request_payload.get("client_message_id"),
        })).await;
        }
        self.wake().await;
        Ok(json!({"ok":true, "queue_id":task.task_id, "priority":1,
            "pause_requested":paused_session.is_some(), "resume_policy":"automatic_at_action_boundary"}))
    }

    pub(super) async fn resume_suspended_tasks(&self) {
        let max_active = self
            .config_store
            .get()
            .await
            .server
            .max_active_sessions
            .max(1);
        while self.active_runtime_session_count() < max_active {
            if self.orchestrator.scheduling.grant_resume().is_none() {
                break;
            }
        }
    }
}
