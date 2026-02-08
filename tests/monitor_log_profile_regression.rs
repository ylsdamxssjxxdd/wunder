use serde_json::{json, Value};
use std::sync::Arc;
use wunder_server::config::{ObservabilityConfig, SandboxConfig};
use wunder_server::monitor::MonitorState;
use wunder_server::storage::{SqliteStorage, StorageBackend};

fn build_monitor(payload_limit: i64) -> MonitorState {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_monitor_profile_it_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let workspace_root = std::env::temp_dir().join(format!(
        "wunder_monitor_profile_ws_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let storage: Arc<dyn StorageBackend> =
        Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let observability = ObservabilityConfig {
        log_level: String::new(),
        monitor_event_limit: 1000,
        monitor_payload_max_chars: payload_limit,
        monitor_drop_event_types: Vec::new(),
    };
    MonitorState::new(
        storage,
        observability,
        SandboxConfig::default(),
        workspace_root.to_string_lossy().to_string(),
    )
}

fn event_types(detail: &Value) -> Vec<String> {
    detail["events"]
        .as_array()
        .expect("events should be an array")
        .iter()
        .map(|event| {
            event["type"]
                .as_str()
                .expect("event type should be a string")
                .to_string()
        })
        .collect()
}

fn find_event<'a>(detail: &'a Value, event_type: &str) -> Option<&'a Value> {
    detail["events"].as_array().and_then(|events| {
        events
            .iter()
            .find(|event| event["type"].as_str() == Some(event_type))
    })
}

#[test]
fn register_records_explicit_user_input_events() {
    let monitor = build_monitor(120);
    let session_id = format!("sess_{}", uuid::Uuid::new_v4().simple());

    let first_question = "first user message";
    let second_question = "second user message";

    assert_eq!(
        monitor.register(&session_id, "user_round", "", first_question, false, false),
        1
    );
    assert_eq!(
        monitor.register(&session_id, "user_round", "", second_question, false, false),
        2
    );

    let detail = monitor
        .get_detail(&session_id)
        .expect("detail should exist");
    let types = event_types(&detail);
    assert_eq!(types[0], "round_start");
    assert_eq!(types[1], "user_input");
    assert_eq!(types[2], "round_start");
    assert_eq!(types[3], "user_input");

    let user_input_events: Vec<&Value> = detail["events"]
        .as_array()
        .expect("events should be an array")
        .iter()
        .filter(|event| event["type"].as_str() == Some("user_input"))
        .collect();
    assert_eq!(user_input_events.len(), 2);

    assert_eq!(
        user_input_events[0]["data"]["message"],
        json!(first_question)
    );
    assert_eq!(
        user_input_events[0]["data"]["question"],
        json!(first_question)
    );
    assert_eq!(user_input_events[0]["data"]["user_round"], json!(1));
    assert_eq!(
        user_input_events[1]["data"]["message"],
        json!(second_question)
    );
    assert_eq!(
        user_input_events[1]["data"]["question"],
        json!(second_question)
    );
    assert_eq!(user_input_events[1]["data"]["user_round"], json!(2));
}

#[test]
fn non_admin_debug_payload_still_uses_normal_profile() {
    let monitor = build_monitor(12);
    let session_id = format!("sess_{}", uuid::Uuid::new_v4().simple());
    monitor.register(&session_id, "user_normal", "", "hello", false, true);
    monitor.record_event(
        &session_id,
        "llm_output_delta",
        &json!({ "delta": "should be skipped" }),
    );
    monitor.record_event(
        &session_id,
        "tool_output_delta",
        &json!({ "delta": "tool should be skipped" }),
    );
    monitor.record_event(
        &session_id,
        "llm_output",
        &json!({ "content": "final output" }),
    );

    let detail = monitor
        .get_detail(&session_id)
        .expect("detail should exist");
    assert_eq!(detail["session"]["log_profile"], json!("normal"));

    let types = event_types(&detail);
    assert!(types.iter().any(|value| value == "llm_output"));
    assert!(!types.iter().any(|value| value == "llm_output_delta"));
    assert!(!types.iter().any(|value| value == "tool_output_delta"));

    let event_ids = detail["events"]
        .as_array()
        .expect("events should be an array")
        .iter()
        .map(|event| event["event_id"].as_i64().expect("event_id should be i64"))
        .collect::<Vec<_>>();
    assert!(event_ids.iter().all(|id| *id > 0));
    assert!(event_ids.windows(2).all(|pair| pair[1] == pair[0] + 1));
}

#[test]
fn admin_debug_profile_keeps_delta_and_full_payload() {
    let monitor = build_monitor(4);
    let session_id = format!("sess_{}", uuid::Uuid::new_v4().simple());
    let long_text = "abcdefghijklmnopqrstuvwxyz";

    monitor.register(&session_id, "admin_user", "", "hello", true, true);
    monitor.record_event(
        &session_id,
        "llm_output_delta",
        &json!({ "delta": long_text }),
    );
    monitor.record_event(
        &session_id,
        "llm_request",
        &json!({ "nested": { "text": long_text } }),
    );

    let detail = monitor
        .get_detail(&session_id)
        .expect("detail should exist");
    assert_eq!(detail["session"]["log_profile"], json!("debug"));

    let delta_event = find_event(&detail, "llm_output_delta").expect("delta event should exist");
    assert_eq!(delta_event["data"]["delta"], json!(long_text));

    let request_event = find_event(&detail, "llm_request").expect("request event should exist");
    assert_eq!(request_event["data"]["nested"]["text"], json!(long_text));
}

#[test]
fn admin_without_debug_payload_uses_normal_profile() {
    let monitor = build_monitor(12);
    let session_id = format!("sess_{}", uuid::Uuid::new_v4().simple());
    monitor.register(&session_id, "admin_user", "", "hello", true, false);
    monitor.record_event(
        &session_id,
        "llm_output_delta",
        &json!({ "delta": "should be skipped" }),
    );

    let detail = monitor
        .get_detail(&session_id)
        .expect("detail should exist");
    assert_eq!(detail["session"]["log_profile"], json!("normal"));
    let types = event_types(&detail);
    assert!(!types.iter().any(|value| value == "llm_output_delta"));
}
