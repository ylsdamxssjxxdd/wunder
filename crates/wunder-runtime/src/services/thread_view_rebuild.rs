//! Canonical projection maintenance has no background worker.
//!
//! Appends advance the bounded projection after rollout flush. Recovery and
//! rebuild are explicit requests, so an idle process owns no history timer.

pub fn snapshot() -> serde_json::Value {
    serde_json::json!({
        "mode": "on_demand",
        "background_enabled": false,
        "work_ms": 0.0,
        "batches": 0,
        "failures": 0,
    })
}
