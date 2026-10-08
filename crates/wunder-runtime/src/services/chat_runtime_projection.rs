//! Resolve chat activity from the live runtime, with bounded durable fallback.
use crate::core::blocking;
use crate::ops::monitor::SessionUsageSummary;
use crate::state::AppState;
use serde_json::Value;

/// The two monitor facts this probe reads. Both a full monitor record and the
/// compact directory summary satisfy it, so a page of threads never has to
/// clone an event tail to resolve activity.
pub trait MonitorStateView {
    fn monitor_status(&self) -> Option<&str>;
    fn monitor_user_rounds(&self) -> i64;
}

impl MonitorStateView for Value {
    fn monitor_status(&self) -> Option<&str> {
        self.get("status").and_then(Value::as_str).map(str::trim)
    }

    fn monitor_user_rounds(&self) -> i64 {
        self.get("user_rounds").and_then(Value::as_i64).unwrap_or(0)
    }
}

impl MonitorStateView for SessionUsageSummary {
    fn monitor_status(&self) -> Option<&str> {
        Some(self.status.as_str())
    }

    fn monitor_user_rounds(&self) -> i64 {
        self.user_rounds
    }
}

pub struct ChatSessionActivity {
    pub runtime: Option<Value>,
    pub running: bool,
}

pub async fn load_chat_session_activity<M: MonitorStateView>(
    state: &AppState,
    session_id: &str,
    monitor: Option<&M>,
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
    let monitor_active = monitor
        .and_then(|value| value.monitor_status())
        .is_some_and(|status| matches!(status, "running" | "cancelling" | "queued" | "waiting"));
    if !monitor_active {
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
    let (turns, hydrated) = blocking::run_db("chat.activity.thread_log", move || {
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
    let status = hydrated
        .as_ref()
        .and_then(|value| value.monitor_status())
        .or_else(|| monitor.and_then(|value| value.monitor_status()))
        .unwrap_or_default();
    let user_rounds = hydrated
        .as_ref()
        .map(|value| value.monitor_user_rounds())
        .or_else(|| monitor.map(|value| value.monitor_user_rounds()))
        .unwrap_or(0);
    ChatSessionActivity {
        runtime: None,
        running: monitor_fallback_is_active(Some(status), user_rounds, &turns),
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

fn monitor_fallback_is_active(
    monitor_status: Option<&str>,
    monitor_rounds: i64,
    turns: &[Value],
) -> bool {
    if !matches!(
        monitor_status.unwrap_or_default(),
        "running" | "cancelling" | "queued" | "waiting"
    ) {
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
    let terminal_round = turn
        .get("user_turn_index")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if terminal_round > 0 && monitor_rounds > terminal_round {
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

    /// Read the monitor facts a directory row carries.
    fn facts(record: &Value) -> (Option<&str>, i64) {
        (record.monitor_status(), record.monitor_user_rounds())
    }

    fn terminal() -> Value {
        json!({"user_turn_index": 2, "status": "completed"})
    }

    #[test]
    fn durable_terminal_overrides_stale_monitor_after_unload() {
        let record = monitor();
        let (status, rounds) = facts(&record);
        assert!(!monitor_fallback_is_active(status, rounds, &[terminal()]));
    }

    #[test]
    fn later_activity_and_new_registration_preserve_running() {
        let record = monitor();
        let (status, rounds) = facts(&record);
        assert!(monitor_fallback_is_active(
            status,
            rounds,
            &[json!({"user_turn_index": 2, "status":"running"})]
        ));
        let mut later_round = monitor();
        later_round["user_rounds"] = json!(3);
        let (status, rounds) = facts(&later_round);
        assert!(monitor_fallback_is_active(status, rounds, &[terminal()]));
    }

    #[test]
    fn waiting_and_missing_evidence_do_not_clear_activity() {
        let record = monitor();
        let (status, rounds) = facts(&record);
        let waiting = json!({"user_turn_index": 2, "status": "waiting_input"});
        assert!(monitor_fallback_is_active(status, rounds, &[waiting]));
        assert!(monitor_fallback_is_active(status, rounds, &[]));
    }

    #[test]
    fn closed_thread_with_timestamp_settles_monitor() {
        let record = monitor();
        let (status, rounds) = facts(&record);
        let closed = json!({"user_turn_index": 2, "status": "completed"});
        assert!(!monitor_fallback_is_active(status, rounds, &[closed]));
        let idle = json!({"user_turn_index": 2, "status": "failed"});
        assert!(!monitor_fallback_is_active(status, rounds, &[idle]));
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
        let record = json!({"status": "finished", "user_rounds": 1});
        let (status, rounds) = facts(&record);
        assert!(!monitor_fallback_is_active(status, rounds, &[]));
    }

    #[test]
    fn directory_summary_carries_the_same_monitor_facts() {
        let summary = SessionUsageSummary {
            status: "waiting".to_string(),
            user_rounds: 4,
            ..Default::default()
        };
        assert_eq!(summary.monitor_status(), Some("waiting"));
        assert_eq!(summary.monitor_user_rounds(), 4);
        let record = json!({"status": "queued", "user_rounds": 7});
        assert_eq!(record.monitor_status(), Some("queued"));
        assert_eq!(record.monitor_user_rounds(), 7);
    }
}
