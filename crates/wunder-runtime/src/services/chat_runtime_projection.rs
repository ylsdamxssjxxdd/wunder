//! Resolve chat activity from the live runtime, with bounded durable fallback.
use crate::core::blocking;
use crate::state::AppState;
use serde_json::Value;

const ACTIVITY_TAIL_LIMIT: i64 = 32;

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

    // Unloaded threads have no in-process snapshot. A monitor row can lag
    // behind turn_terminal; inspect only the durable tail, never full history.
    let storage = state.storage.clone();
    let monitor_service = state.monitor.clone();
    let session = session_id.to_string();
    let (events, current_monitor) = blocking::run_db("chat.activity.tail", move || {
        let events = storage.load_recent_stream_events(&session, ACTIVITY_TAIL_LIMIT)?;
        // This lookup may hydrate storage when the monitor row is not cached.
        Ok((events, monitor_service.get_record(&session)))
    })
    .await
    .unwrap_or_default();
    // A new turn may have started while the storage read was in flight.
    if let Some(runtime) = snapshot() {
        return from_runtime(runtime);
    }
    let running = monitor_fallback_is_active(current_monitor.as_ref().or(monitor), &events);
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

fn event_data(event: &Value) -> &Value {
    let envelope = event.get("data").unwrap_or(event);
    envelope.get("data").unwrap_or(envelope)
}

fn event_activity(event: &Value) -> Option<bool> {
    let data = event_data(event);
    match event
        .get("type")
        .or_else(|| event.get("event"))
        .and_then(Value::as_str)?
    {
        "thread_closed" => Some(false),
        "thread_status" => match data
            .get("thread_status")
            .or_else(|| data.get("status"))
            .and_then(Value::as_str)
        {
            Some("idle" | "not_loaded" | "completed" | "failed" | "cancelled" | "system_error") => {
                Some(false)
            }
            Some("running" | "queued" | "waiting_approval" | "waiting_user_input") => Some(true),
            _ => None,
        },
        "turn_terminal" => {
            if data.get("waiting_for_user_input").and_then(Value::as_bool) == Some(true) {
                return Some(true);
            }
            match data.get("status").and_then(Value::as_str) {
                Some("completed" | "failed" | "cancelled" | "error") => Some(false),
                _ => None,
            }
        }
        // These events can belong to a later turn whose runtime is on another
        // process. Do not let an earlier terminal event hide that activity.
        "round_start" | "received" | "user_input" | "queue_start" | "progress" | "llm_request"
        | "llm_response" | "llm_output" | "llm_output_delta" | "tool_call" | "tool_result"
        | "compaction" | "approval_request" | "approval_result" | "question_panel" => Some(true),
        _ => None,
    }
}

fn timestamp(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    value.as_f64().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()
            .map(|time| time.timestamp_millis() as f64 / 1000.0)
    })
}

fn monitor_fallback_is_active(monitor: Option<&Value>, events: &[Value]) -> bool {
    if !monitor_is_active(monitor) {
        return false;
    }
    let Some(terminal) = events
        .iter()
        .rev()
        .find_map(|event| event_activity(event).map(|active| (event, active)))
    else {
        return true;
    };
    if terminal.1 {
        return true;
    }
    let terminal = terminal.0;
    let monitor = monitor.expect("active monitor exists");
    let current_round = monitor
        .get("user_rounds")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let terminal_round = event_data(terminal)
        .get("user_round")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if terminal_round > 0 && current_round > terminal_round {
        return true;
    }
    // Registration precedes its first persisted event. Preserve that new run
    // even when the durable tail still ends at the previous turn's terminal.
    let terminal_time = timestamp(terminal.get("timestamp"))
        .or_else(|| timestamp(terminal.get("data").and_then(|data| data.get("timestamp"))));
    if let (Some(start), Some(end)) = (timestamp(monitor.get("start_time")), terminal_time) {
        if start > end {
            return true;
        }
    } else if terminal_round == 0 || current_round == 0 {
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
        json!({"event": "turn_terminal", "timestamp": 12.0,
            "data": {"data": {"user_round": 2, "status": "completed"}}})
    }

    #[test]
    fn durable_terminal_overrides_stale_monitor_after_unload() {
        let events = vec![terminal(), json!({"event": "context_usage"})];
        assert!(!monitor_fallback_is_active(Some(&monitor()), &events));
    }

    #[test]
    fn later_activity_and_new_registration_preserve_running() {
        let mut events = vec![terminal()];
        events.push(json!({"event": "llm_request", "timestamp": 14.0}));
        assert!(monitor_fallback_is_active(Some(&monitor()), &events));
        let mut later_round = monitor();
        later_round["user_rounds"] = json!(3);
        assert!(monitor_fallback_is_active(
            Some(&later_round),
            &[terminal()]
        ));
        let later_start = json!({"status": "running", "user_rounds": 2, "start_time": 13.0});
        assert!(monitor_fallback_is_active(
            Some(&later_start),
            &[terminal()]
        ));
    }

    #[test]
    fn waiting_and_missing_evidence_do_not_clear_activity() {
        let waiting = json!({"event": "turn_terminal", "timestamp": 12.0,
            "data": {"status": "completed", "waiting_for_user_input": true}});
        assert!(monitor_fallback_is_active(Some(&monitor()), &[waiting]));
        assert!(monitor_fallback_is_active(Some(&monitor()), &[]));
        assert!(monitor_fallback_is_active(
            Some(&monitor()),
            &[json!({"event": "thread_closed"})]
        ));
    }

    #[test]
    fn closed_thread_with_timestamp_settles_monitor() {
        let closed = json!({"event": "thread_closed", "timestamp": "1970-01-01T00:00:12Z"});
        assert!(!monitor_fallback_is_active(Some(&monitor()), &[closed]));
        let idle = json!({"event": "thread_status", "timestamp": 12.0, "data": {"status": "idle"}});
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
