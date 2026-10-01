//! Resolve chat activity from the live runtime, with bounded durable fallback.
use crate::core::blocking;
use crate::state::AppState;
use serde_json::Value;

pub struct ChatSessionActivity {
    pub runtime: Option<Value>,
    pub running: bool,
}

pub async fn load_chat_session_activity(
    state: &AppState,
    session_id: &str,
    monitor: Option<&Value>,
) -> ChatSessionActivity {
    let snapshot = || {
        state
            .kernel
            .orchestrator
            .get_tool_session_runtime_snapshot(session_id)
    };
    if let Some(runtime) = snapshot() {
        return from_runtime(runtime);
    }
    if !monitor_is_active(monitor) {
        return ChatSessionActivity {
            runtime: None,
            running: false,
        };
    }

    // Unloaded threads have no in-process snapshot. A monitor row can lag a
    // terminal transition, so inspect the durable ThreadLog turn state rather
    // than the compatibility stream-event tail.
    let storage = state.storage.clone();
    let monitor_service = state.monitor.clone();
    let session = session_id.to_string();
    let (turns, current_monitor) = blocking::run_db("chat.activity.thread_log", move || {
        let owner = storage.get_chat_session_owner(&session)?;
        let turns = owner
            .as_deref()
            .map(|user_id| storage.list_thread_turns(user_id, &session, None, 1))
            .transpose()?
            .unwrap_or_default();
        // This lookup may hydrate storage when the monitor row is not cached.
        Ok((turns, monitor_service.get_record(&session)))
    })
    .await
    .unwrap_or_default();
    // A new turn may have started while the storage read was in flight.
    if let Some(runtime) = snapshot() {
        return from_runtime(runtime);
    }
    let running = monitor_fallback_is_active(current_monitor.as_ref().or(monitor), &turns);
    ChatSessionActivity {
        runtime: None,
        running,
    }
}

fn from_runtime(runtime: Value) -> ChatSessionActivity {
    let running = matches!(
        runtime.get("thread_status").and_then(Value::as_str),
        Some("running" | "waiting_approval" | "waiting_user_input")
    );
    ChatSessionActivity {
        runtime: Some(runtime),
        running,
    }
}

fn monitor_is_active(monitor: Option<&Value>) -> bool {
    matches!(
        monitor
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("running" | "cancelling" | "queued" | "waiting")
    )
}

fn monitor_fallback_is_active(monitor: Option<&Value>, turns: &[Value]) -> bool {
    if !monitor_is_active(monitor) {
        return false;
    }
    let Some(turn) = turns.first() else {
        return true;
    };
    let status = turn.get("status").and_then(Value::as_str).unwrap_or("");
    if matches!(status, "queued" | "running" | "waiting_input") {
        return true;
    }
    if !matches!(
        status,
        "completed" | "failed" | "cancelled" | "interrupted" | "rejected" | "stopped"
    ) {
        return true;
    }
    let monitor = monitor.expect("active monitor exists");
    let current_round = monitor
        .get("user_rounds")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let terminal_round = turn
        .get("user_turn_index")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if terminal_round > 0 && current_round > terminal_round {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn monitor() -> Value {
        json!({"status": "running", "user_rounds": 2, "start_time": 10.0})
    }

    fn terminal() -> Value {
        json!({"user_turn_index": 2, "status": "completed"})
    }

    #[test]
    fn durable_terminal_overrides_stale_monitor_after_unload() {
        assert!(!monitor_fallback_is_active(Some(&monitor()), &[terminal()]));
    }

    #[test]
    fn later_activity_and_new_registration_preserve_running() {
        assert!(monitor_fallback_is_active(
            Some(&monitor()),
            &[json!({"user_turn_index": 2, "status":"running"})]
        ));
        let mut later_round = monitor();
        later_round["user_rounds"] = json!(3);
        assert!(monitor_fallback_is_active(
            Some(&later_round),
            &[terminal()]
        ));
    }

    #[test]
    fn waiting_and_missing_evidence_do_not_clear_activity() {
        let waiting = json!({"user_turn_index": 2, "status": "waiting_input"});
        assert!(monitor_fallback_is_active(Some(&monitor()), &[waiting]));
        assert!(monitor_fallback_is_active(Some(&monitor()), &[]));
    }

    #[test]
    fn closed_thread_with_timestamp_settles_monitor() {
        let closed = json!({"user_turn_index": 2, "status": "completed"});
        assert!(!monitor_fallback_is_active(Some(&monitor()), &[closed]));
        let idle = json!({"user_turn_index": 2, "status": "failed"});
        assert!(!monitor_fallback_is_active(Some(&monitor()), &[idle]));
    }

    #[test]
    fn runtime_snapshot_retains_live_waiting_and_idle_authority() {
        for (status, expected) in [
            ("running", true),
            ("waiting_approval", true),
            ("waiting_user_input", true),
            ("idle", false),
            ("queued", false),
        ] {
            assert_eq!(
                from_runtime(json!({"thread_status": status})).running,
                expected
            );
        }
        assert!(!monitor_fallback_is_active(
            Some(&json!({"status": "finished"})),
            &[]
        ));
    }
}
