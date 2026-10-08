use crate::services::runtime::thread::mailbox::MAX_MESSAGE_BYTES;
use serde_json::{json, Map, Value};

// Large results remain in the child thread. The wake contains enough references
// to retrieve them without letting one completion monopolize the parent context.
pub(super) fn bounded_completion(payload: Value) -> Value {
    if serde_json::to_vec(&payload).is_ok_and(|bytes| bytes.len() < MAX_MESSAGE_BYTES - 128) {
        return payload;
    }
    let dispatch = &payload["dispatch"];
    let mut compact = compact_item(dispatch);
    for key in [
        "total",
        "done_total",
        "success_total",
        "failed_total",
        "all_finished",
        "completion_reached",
    ] {
        if let Some(value) = dispatch
            .get(key)
            .filter(|value| value.is_number() || value.is_boolean())
        {
            compact[key] = value.clone();
        }
    }
    if let Some(items) = dispatch.get("items").and_then(Value::as_array) {
        compact["items"] = items.iter().take(3).map(compact_item).collect();
        compact["items_omitted"] = json!(items.len().saturating_sub(3));
    }
    json!({
        "kind":"subagent_auto_wake", "truncated":true,
        "instruction":"Background work completed. This is a compact notice; use subagent_control status/history with the dispatch or session references to retrieve full results.",
        "dispatch":compact
    })
}

fn compact_item(item: &Value) -> Value {
    let mut result = Map::new();
    for key in [
        "session_id",
        "parent_session_id",
        "run_id",
        "dispatch_id",
        "status",
        "summary",
    ] {
        if let Some(text) = item.get(key).and_then(Value::as_str) {
            // Keep identifiers intact. Four records with six bounded fields each
            // still fit below 20 KB even with maximal JSON escaping.
            if key == "summary" {
                result.insert(key.into(), Value::String(text.chars().take(128).collect()));
            } else if text.len() <= 128 {
                result.insert(key.into(), Value::String(text.to_string()));
            }
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_message_completion_compacts_large_results_preserving_references() {
        let payload = json!({"dispatch":{"run_id":"run", "dispatch_id":"dispatch", "session_id":"child",
            "summary":"字".repeat(30_000),"result":{"answer":"x".repeat(30_000)}}});
        let compact = bounded_completion(payload);
        assert_eq!(compact["dispatch"]["run_id"], "run");
        assert_eq!(compact["dispatch"]["session_id"], "child");
        assert_eq!(compact["dispatch"]["dispatch_id"], "dispatch");
        assert_eq!(compact["truncated"], true);
        assert!(serde_json::to_vec(&compact).unwrap().len() < MAX_MESSAGE_BYTES - 128);
        assert!(compact["dispatch"].get("result").is_none());
    }

    #[test]
    fn agent_message_completion_bounds_escaped_batch_and_retains_small_payload() {
        let small = json!({"dispatch":{"run_id":"run","status":"success"}});
        assert_eq!(bounded_completion(small.clone()), small);
        let item = json!({"session_id":"child","run_id":"run","dispatch_id":"dispatch",
            "parent_session_id":"parent","status":"success","summary":"\u{0000}".repeat(30_000)});
        let compact = bounded_completion(json!({"dispatch":{"items":vec![item;32]}}));
        assert_eq!(compact["dispatch"]["items_omitted"], 29);
        assert!(serde_json::to_vec(&compact).unwrap().len() < MAX_MESSAGE_BYTES - 128);
    }
}
