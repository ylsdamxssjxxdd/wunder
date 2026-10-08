//! Stable simulation profiles shared by model configuration and all runtimes.
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Provider capabilities shared by synthetic responses and recorded replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
// Ignore removed prototype fields when loading existing configs; serialization drops them.
#[serde(default)]
pub struct VirtualModelOptions {
    pub support_tools: bool,
    pub support_reasoning: bool,
    pub image_tokens: u32,
    pub audio_tokens: u32,
}

impl Default for VirtualModelOptions {
    fn default() -> Self {
        Self {
            support_tools: true,
            support_reasoning: true,
            image_tokens: 256,
            audio_tokens: 1024,
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
