//! Stable simulation profiles shared by model configuration and all runtimes.
use serde::{Deserialize, Serialize};
use std::time::Duration;

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
