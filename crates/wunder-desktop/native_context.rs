//! Context occupancy is independent from billing and cumulative output tokens.
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;
use wunder_server::{blocking, state::AppState};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeContextUsage {
    pub used: Option<f64>,
    pub capacity: Option<f64>,
}

impl NativeContextUsage {
    /// Normalize current runtime events and persisted message stats.
    /// Never fall back to billing totals or prompt estimates.
    pub fn from_value(value: &Value) -> Self {
        fn number(value: &Value, keys: &[&str]) -> Option<f64> {
            keys.iter()
                .filter_map(|key| value.get(*key))
                .find_map(|value| {
                    value
                        .as_f64()
                        .or_else(|| value.as_str()?.parse().ok())
                        .filter(|value| value.is_finite() && *value >= 0.0)
                })
        }
        let envelope = value.get("data").unwrap_or(value);
        let event_type = value
            .get("event")
            .or_else(|| value.get("type"))
            .and_then(Value::as_str);
        let data = if matches!(event_type, Some("context_usage" | "round_usage")) {
            envelope
        } else {
            envelope.get("data").unwrap_or(envelope)
        };
        let stats = data
            .get("stats")
            .or_else(|| data.get("message_stats"))
            .or_else(|| data.pointer("/meta/message_stats"))
            .unwrap_or(data);
        let nested = stats
            .get("context_usage")
            .or_else(|| stats.get("contextUsage"));
        let used_keys = [
            "context_occupancy_tokens",
            "contextOccupancyTokens",
            "context_tokens",
            "contextTokens",
        ];
        let capacity_keys = [
            "max_context",
            "maxContext",
            "context_max_tokens",
            "contextMaxTokens",
            "context_total_tokens",
            "contextTotalTokens",
            "context_window_tokens",
            "context_limit",
            "max_context_tokens",
        ];
        Self {
            used: number(stats, &used_keys).or_else(|| nested.and_then(|v| number(v, &used_keys))),
            capacity: number(stats, &capacity_keys)
                .or_else(|| nested.and_then(|v| number(v, &capacity_keys)))
                .filter(|v| *v > 0.0),
        }
    }
}

pub(super) async fn snapshot(
    state: &Arc<AppState>,
    user: &str,
    session: &str,
) -> Result<NativeContextUsage> {
    let db = state.clone();
    let owner = user.to_owned();
    let target = session.to_owned();
    let used = blocking::run_db("native.context.snapshot", move || {
        db.user_store
            .get_chat_session(&owner, &target)?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        Ok(db.workspace.load_session_context_tokens(&owner, &target))
    })
    .await?;
    let capacity = wunder_server::api::chat::native_chat_context_capacity(state, user, session)
        .await?
        .map(f64::from);
    Ok(NativeContextUsage {
        used: Some(used.max(0) as f64),
        capacity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn runtime_occupancy_uses_max_context_and_accepts_zero_after_compaction() {
        let usage = NativeContextUsage::from_value(
            &json!({"data":{"context_occupancy_tokens":2400,"max_context":8000}}),
        );
        assert_eq!(
            usage,
            NativeContextUsage {
                used: Some(2400.0),
                capacity: Some(8000.0)
            }
        );
        assert_eq!(
            NativeContextUsage::from_value(&json!({"context_tokens":0,"max_context":8000})).used,
            Some(0.0)
        );
    }
    #[test]
    fn persisted_aliases_and_numeric_strings_do_not_use_billing_totals() {
        let usage = NativeContextUsage::from_value(
            &json!({"stats":{"contextUsage":{"contextOccupancyTokens":"3200","context_max_tokens":16000},"total_tokens":99999}}),
        );
        assert_eq!(
            usage,
            NativeContextUsage {
                used: Some(3200.0),
                capacity: Some(16000.0)
            }
        );
        assert_eq!(
            NativeContextUsage::from_value(
                &json!({"usage":{"prompt_tokens":900,"total_tokens":1000}})
            ),
            NativeContextUsage::default()
        );
    }
}
