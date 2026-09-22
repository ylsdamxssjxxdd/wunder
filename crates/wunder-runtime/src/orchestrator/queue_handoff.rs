use super::thread_runtime::ThreadRuntimeStatus;
use super::*;

impl Orchestrator {
    /// Park the existing future between actions: no tool replay and no extra user turn.
    pub(super) async fn yield_queue_slot(
        &self,
        session_id: &str,
        emitter: &EventEmitter,
        round: RoundInfo,
    ) -> Result<(), OrchestratorError> {
        if !self.scheduling.pause_requested(session_id) {
            return Ok(());
        }
        self.ensure_not_cancelled(session_id)?;
        // Flush durable output before releasing capacity; browser replay can restore the boundary.
        flush_stream_event_persist_queue().await;
        let storage = self.storage.clone();
        let id = session_id.to_string();
        let suspended = crate::core::blocking::run_db("queue.suspend_lock", move || {
            storage.set_session_lock_suspended(&id, true, 1)
        })
        .await
        .map_err(|err| OrchestratorError::internal(err.to_string()))?;
        if !suspended {
            return Err(OrchestratorError::internal(
                "session lock unavailable".into(),
            ));
        }
        self.scheduling.mark_suspended(session_id);
        self.monitor.mark_queued(session_id, None);
        let turn_id = self
            .thread_runtime
            .snapshot(session_id)
            .and_then(|snapshot| snapshot.active_turn_id);
        if let Some(turn_id) = turn_id.as_deref() {
            self.emit_thread_runtime_update(
                emitter,
                round,
                self.thread_runtime
                    .set_status(session_id, turn_id, ThreadRuntimeStatus::Queued),
            )
            .await;
        }
        let mut payload = json!({"session_id":session_id, "reason":"admin_preempted", "resumable":true, "queue_state":"suspended"});
        round.insert_into(payload.as_object_mut().unwrap());
        emitter.emit("queue_enter", payload).await;
        loop {
            self.ensure_not_cancelled(session_id)?;
            if self.scheduling.resume_granted(session_id) {
                let max_active = self
                    .config_store
                    .get()
                    .await
                    .server
                    .max_active_sessions
                    .max(1) as i64;
                let storage = self.storage.clone();
                let id = session_id.to_string();
                let resumed = crate::core::blocking::run_db("queue.resume_lock", move || {
                    storage.set_session_lock_suspended(&id, false, max_active)
                })
                .await
                .map_err(|err| OrchestratorError::internal(err.to_string()))?;
                if resumed {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        self.scheduling.mark_resumed(session_id);
        self.monitor.mark_running(session_id, None);
        let mut payload = json!({"session_id":session_id, "reason":"admin_preempted", "resumed":true, "queue_state":"resumed"});
        round.insert_into(payload.as_object_mut().unwrap());
        emitter.emit("queue_start", payload).await;
        if let Some(turn_id) = turn_id.as_deref() {
            self.emit_thread_runtime_update(
                emitter,
                round,
                self.thread_runtime
                    .set_status(session_id, turn_id, ThreadRuntimeStatus::Running),
            )
            .await;
        }
        Ok(())
    }
}
