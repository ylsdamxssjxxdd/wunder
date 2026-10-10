//! Stable simulation profiles shared by model configuration and all runtimes.
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default probability that a synthetic (random-simulation) turn fails with a
/// transient provider error before producing any output. Kept tiny so the
/// out-of-the-box simulation still looks realistic; tests that assert exact
/// behavior pin both fault rates to 0.0.
pub const DEFAULT_ERROR_RATE: f64 = 0.01;
/// Default probability that a synthetic stream drops mid-answer, simulating a
/// brief, recoverable disconnect.
pub const DEFAULT_DISCONNECT_RATE: f64 = 0.005;

/// Provider capabilities shared by synthetic responses and recorded replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
// Ignore removed prototype fields when loading existing configs; serialization drops them.
#[serde(default)]
pub struct VirtualModelOptions {
    pub support_tools: bool,
    pub support_reasoning: bool,
    pub image_tokens: u32,
    pub audio_tokens: u32,
    /// Probability (0.0..=1.0) of a transient provider error before any output
    /// (simulated 5xx / rate limit). Applies only to the random-simulation path;
    /// recorded replay stays faithful to what was captured.
    pub error_rate: f64,
    /// Probability (0.0..=1.0) that a synthetic stream drops mid-answer,
    /// simulating a brief, recoverable disconnect that the client retries.
    pub disconnect_rate: f64,
}

impl Default for VirtualModelOptions {
    fn default() -> Self {
        Self {
            support_tools: true,
            support_reasoning: true,
            image_tokens: 256,
            audio_tokens: 1024,
            error_rate: DEFAULT_ERROR_RATE,
            disconnect_rate: DEFAULT_DISCONNECT_RATE,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VirtualModelSpeed {
    #[default]
    Fast,
    Medium,
    Slow,
}

impl VirtualModelSpeed {
    pub const fn prefill_tokens_per_second(self) -> u64 {
        match self {
            Self::Fast => 2000,
            Self::Medium => 500,
            Self::Slow => 100,
        }
    }
    pub const fn generation_tokens_per_second(self) -> u64 {
        match self {
            Self::Fast => 200,
            Self::Medium => 50,
            Self::Slow => 10,
        }
    }
    pub fn prefill_duration(self, tokens: u64) -> Duration {
        Duration::from_secs_f64(tokens as f64 / self.prefill_tokens_per_second() as f64)
    }
    pub fn generation_duration(self, tokens: u64) -> Duration {
        Duration::from_secs_f64(tokens as f64 / self.generation_tokens_per_second() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_fault_rates_are_tiny_but_nonzero() {
        let options = VirtualModelOptions::default();
        assert!(options.error_rate > 0.0 && options.error_rate < 0.05);
        assert!(options.disconnect_rate > 0.0 && options.disconnect_rate < 0.05);
    }

    #[test]
    fn default_capabilities_unchanged() {
        let options = VirtualModelOptions::default();
        assert!(options.support_tools);
        assert!(options.support_reasoning);
        assert_eq!(options.image_tokens, 256);
        assert_eq!(options.audio_tokens, 1024);
    }
}
