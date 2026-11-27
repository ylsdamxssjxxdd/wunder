//! Context occupancy is independent from billing and cumulative output tokens.
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;
use wunder_server::{blocking, state::AppState};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NativeContextUsage {
    pub used: Option<f64>,
    pub capacity: Option<f64>,
    /// True once this thread's model context has been replaced by a compaction
    /// summary. The composer paints its gauge red from then on so the user can
    /// tell a compacted thread from a fresh one at a glance.
    pub compacted: bool,
}

/// The composition of the thread's last outgoing request, as the engine
/// recorded it at the request gate. Empty parts mean this thread never sent a
/// request yet; the shells then show the window without a breakdown.
#[derive(Clone, Debug, Default)]
pub struct NativeContextWindow {
    pub model: String,
    pub provider: String,
    /// The model's context window, 0 when unknown.
    pub window: i64,
    pub tokens: i64,
    /// True when `tokens` is the vendor's count rather than a local estimate.
    pub measured: bool,
    pub turns: i64,
    pub parts: Vec<wunder_server::context_window::ContextPart>,
}

impl super::NativeDesktop {
    /// Used/capacity for one thread, independent of billing totals. The
    /// Android shell reads this after opening a thread, switching models and
    /// settling a turn so the brain gauge survives process restarts.
    pub fn context_usage(&self, session: &str) -> Result<NativeContextUsage> {
        self.runtime
            .block_on(snapshot(self.state(), self.user_id(), session))
    }

    /// The text of one row in the thread's last request. Memory only, so this
    /// is deliberately synchronous and never touches the database.
    pub fn context_text(
        &self,
        session: &str,
        slot: i32,
        tag: &str,
        name: &str,
        count: i64,
    ) -> Option<String> {
        self.state()
            .workspace
            .context_text(self.user_id(), session, slot, tag, name, count)
    }

    /// What the thread's last request was made of. The context card is opened
    /// from the composer's model trigger, so this has to answer for a thread
    /// that has never sent anything (window only) as well as for one that has.
    pub fn context_window(&self, session: &str) -> Result<NativeContextWindow> {
        self.runtime.block_on(async {
            let state = self.state();
            let user = self.user_id().to_string();
            let target = session.to_string();
            let db = state.clone();
            let owner = user.clone();
            let thread = target.clone();
            let recorded = blocking::run_db("native.context.window", move || {
                db.user_store
                    .get_chat_session(&owner, &thread)?
                    .ok_or_else(|| anyhow!("chat session not found"))?;
                Ok(db.workspace.load_session_context_window(&owner, &thread))
            })
            .await?;
            let capacity = wunder_server::api::chat::native_chat_context_capacity(
                &state, &user, &target,
            )
            .await?
            .map(i64::from)
            .unwrap_or(0);
            Ok(match recorded {
                Some(recorded) => NativeContextWindow {
                    model: recorded.model,
                    provider: recorded.provider,
                    window: if recorded.window > 0 {
                        recorded.window
                    } else {
                        capacity
                    },
                    tokens: recorded.tokens,
                    measured: recorded.measured,
                    turns: recorded.turns,
                    parts: recorded.parts,
                },
                None => NativeContextWindow {
                    window: capacity,
                    ..Default::default()
                },
            })
        })
    }
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
            compacted: false,
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
    let db = state.clone();
    let owner = user.to_owned();
    let target = session.to_owned();
    let compacted = blocking::run_db("native.context.compacted", move || {
        Ok(db.workspace.load_session_compacted(&owner, &target))
    })
    .await
    .unwrap_or(false);
    let capacity = wunder_server::api::chat::native_chat_context_capacity(state, user, session)
        .await?
        .map(f64::from);
    Ok(NativeContextUsage {
        // Zero means "never counted by the provider" — a fresh thread, or the
        // history that a compaction just replaced. Reporting it as an occupancy
        // drew 0 / max on the gauge while the real prompt already held the
        // system prompt and every tool definition, so the shells show "unknown"
        // until the first request publishes a measurement.
        used: (used > 0).then_some(used as f64),
        capacity,
        compacted,
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
                capacity: Some(8000.0),
                compacted: false
            }
        );
        assert_eq!(
            NativeContextUsage::from_value(&json!({"context_tokens":0,"max_context":8000})).used,
            Some(0.0)
        );
    }
    #[test]
    fn unobserved_zero_yields_to_the_measured_request_projection() {
        // The pre-request frame of a fresh or freshly compacted thread carries
        // no provider count, only the measured outgoing request: reading the
        // zero instead drew an empty gauge over a prompt that already held the
        // system prompt and the tool definitions.
        let usage = NativeContextUsage::from_value(&json!({
            "context_tokens": 0,
            "context_occupancy_tokens": 13000,
            "context_usage_source": "request_projection",
            "max_context": 400000
        }));
        assert_eq!(usage.used, Some(13000.0));
        assert_eq!(usage.capacity, Some(400000.0));
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
                capacity: Some(16000.0),
                compacted: false
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
