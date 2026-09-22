//! Replay batches overlap live deltas: deduplicate each persisted segment.
use serde_json::{json, Value};

pub struct EventCursor {
    pub last_id: i64,
}
impl EventCursor {
    pub fn new(last_id: i64) -> Self {
        Self { last_id }
    }
    pub fn accept(&mut self, payload: &Value, session: &str) -> Vec<Value> {
        let kind = payload["event"].as_str().unwrap_or("");
        let data = &payload["data"];
        if data["session_id"].as_str().is_some_and(|id| id != session) {
            return Vec::new();
        }
        if kind == "heartbeat" {
            return Vec::new();
        }
        let id = number(&payload["id"]).max(number(&data["event_id"]));
        if id > 0 && id <= self.last_id {
            return Vec::new();
        }
        let mut events = Vec::new();
        if let Some(segments) = data["segments"].as_array() {
            for segment in segments {
                let segment_id = number(&segment["event_id"]);
                if segment_id > self.last_id {
                    events.push(json!({"event":kind,"data":segment}));
                    self.last_id = segment_id;
                }
            }
        } else {
            events.push(json!({"event":kind,"data":data}));
        }
        self.last_id = self.last_id.max(id);
        events
    }
}
fn number(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_str()?.parse().ok())
        .unwrap_or(0)
}
pub fn is_terminal(event: &Value) -> bool {
    matches!(
        event["event"].as_str(),
        // `final` carries the answer but the runtime persists the assistant
        // transcript and emits `turn_terminal` immediately afterwards. Keep
        // the socket alive through that settlement event so history refreshes
        // cannot race the durable write.
        Some("turn_terminal" | "error" | "queue_fail" | "queue_finish")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_replay_keeps_only_unseen_segments() {
        let mut cursor = EventCursor::new(4);
        let batch = json!({"event":"llm_output_delta","id":"7","data":{"segments":[
            {"event_id":4,"delta":"a"},{"event_id":6,"delta":"b"},{"event_id":7,"delta":"中"}]}});
        assert_eq!(
            cursor.accept(&batch, "test"),
            vec![
                json!({"event":"llm_output_delta","data":{"event_id":6,"delta":"b"}}),
                json!({"event":"llm_output_delta","data":{"event_id":7,"delta":"中"}})
            ]
        );
        assert_eq!(cursor.accept(&batch, "test"), Vec::<Value>::new());
        assert_eq!(
            cursor.accept(
                &json!({"event":"final","id":9,"data":{"session_id":"other"}}),
                "test"
            ),
            Vec::<Value>::new()
        );
        assert_eq!(cursor.last_id, 7);
    }
}
