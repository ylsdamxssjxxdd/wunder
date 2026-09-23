//! Deterministic timing shared by replay and synthetic throughput benchmarks.
use serde_json::Value;
pub use wunder_core::virtual_model::VirtualModelSpeed;

pub fn input_tokens(messages: &[Value]) -> u64 {
    crate::token_utils::estimate_messages_tokens(messages).max(0) as u64
}

pub async fn wait_for_prefill(input_tokens: u64, speed: VirtualModelSpeed) {
    tokio::time::sleep(speed.prefill_duration(input_tokens)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn profiles_match_requested_rates_and_default_is_fast() {
        use VirtualModelSpeed::{Fast, Medium, Slow};
        assert_eq!(VirtualModelSpeed::default(), Fast);
        assert_eq!(
            [Fast, Medium, Slow]
                .map(|speed| (speed.prefill_duration(2000), speed.generation_duration(200))),
            [
                (Duration::from_secs(1), Duration::from_secs(1)),
                (Duration::from_secs(4), Duration::from_secs(4)),
                (Duration::from_secs(20), Duration::from_secs(20))
            ]
        );
        assert!(serde_json::from_str::<VirtualModelSpeed>(r#""invalid""#).is_err());
        assert_eq!(
            input_tokens(&[serde_json::json!({"role":"user","content":"text"})]),
            5
        );
    }
}
