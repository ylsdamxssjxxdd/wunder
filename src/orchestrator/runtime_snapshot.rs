use super::thread_runtime::ThreadRuntimeStatus;
use super::*;

impl Orchestrator {
    pub fn get_tool_session_runtime_snapshot(&self, session_id: &str) -> Option<Value> {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return None;
        }

        let thread_snapshot = self.thread_runtime.snapshot(cleaned_session);
        let active_turn_snapshot = self.active_turns.snapshot(cleaned_session);
        if thread_snapshot.is_none() && active_turn_snapshot.is_none() {
            return None;
        }

        let derived_active_status = active_turn_snapshot.as_ref().map(|snapshot| {
            if snapshot.waiting_for_user_input {
                ThreadRuntimeStatus::WaitingUserInput
            } else if !snapshot.pending_approval_ids.is_empty() {
                ThreadRuntimeStatus::WaitingApproval
            } else {
                ThreadRuntimeStatus::Running
            }
        });
        let thread_status = thread_snapshot
            .as_ref()
            .map(|snapshot| snapshot.status.as_str().to_string())
            .or_else(|| derived_active_status.map(|status| status.as_str().to_string()))
            .unwrap_or_else(|| ThreadRuntimeStatus::NotLoaded.as_str().to_string());
        let subscriber_count = thread_snapshot
            .as_ref()
            .map(|snapshot| snapshot.subscriber_count)
            .unwrap_or(0);
        let runtime_active_turn_id = thread_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.active_turn_id.clone());

        let turn_id = active_turn_snapshot
            .as_ref()
            .map(|snapshot| snapshot.turn_id.clone())
            .or_else(|| runtime_active_turn_id.clone());
        let pending_approval_ids = active_turn_snapshot
            .as_ref()
            .map(|snapshot| snapshot.pending_approval_ids.clone())
            .unwrap_or_default();
        let pending_approval_count = pending_approval_ids.len();
        let waiting_for_user_input = active_turn_snapshot
            .as_ref()
            .map(|snapshot| snapshot.waiting_for_user_input)
            .unwrap_or(false);

        Some(json!({
            "session_id": cleaned_session,
            "thread_status": thread_status,
            "loaded": thread_snapshot.is_some() || active_turn_snapshot.is_some(),
            "subscriber_count": subscriber_count,
            "active_turn_id": turn_id,
            "turn": {
                "turn_id": turn_id,
                "pending_approval_ids": pending_approval_ids,
                "pending_approval_count": pending_approval_count,
                "waiting_for_user_input": waiting_for_user_input,
            },
        }))
    }
}
