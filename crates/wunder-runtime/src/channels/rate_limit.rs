use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::time::Instant;

use parking_lot::Mutex;

#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    pub qps: u32,
    pub concurrency: u32,
}

#[derive(Debug)]
struct RateState {
    tokens: f64,
    last_refill: Instant,
    in_flight: u32,
}

impl RateState {
    fn new() -> Self {
        Self {
            tokens: 0.0,
            last_refill: Instant::now(),
            in_flight: 0,
        }
    }
}

#[derive(Clone)]
pub struct ChannelRateLimiter {
    states: std::sync::Arc<Mutex<HashMap<String, RateState>>>,
}

pub struct RateLimitGuard {
    key: String,
    limiter: ChannelRateLimiter,
}

impl ChannelRateLimiter {
    pub fn new() -> Self {
        Self {
            states: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn acquire(&self, key: &str, cfg: RateLimitConfig) -> Option<RateLimitGuard> {
        if cfg.qps == 0 && cfg.concurrency == 0 {
            return Some(RateLimitGuard {
                key: key.to_string(),
                limiter: self.clone(),
            });
        }
        let mut states = self.states.lock();
        let (state, is_new) = match states.entry(key.to_string()) {
            Entry::Vacant(entry) => (entry.insert(RateState::new()), true),
            Entry::Occupied(entry) => (entry.into_mut(), false),
        };
        if is_new && cfg.qps > 0 {
            state.tokens = cfg.qps as f64;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(state.last_refill);
        if cfg.qps > 0 {
            let refill = elapsed.as_secs_f64() * cfg.qps as f64;
            let capacity = cfg.qps as f64;
            state.tokens = (state.tokens + refill).min(capacity);
        } else {
            state.tokens = f64::INFINITY;
        }
        state.last_refill = now;
        if cfg.concurrency > 0 && state.in_flight >= cfg.concurrency {
            return None;
        }
        if cfg.qps > 0 && state.tokens < 1.0 {
            return None;
        }
        if cfg.qps > 0 {
            state.tokens -= 1.0;
        }
        state.in_flight = state.in_flight.saturating_add(1);
        Some(RateLimitGuard {
            key: key.to_string(),
            limiter: self.clone(),
        })
    }
}

impl Drop for RateLimitGuard {
    fn drop(&mut self) {
        let mut states = self.limiter.states.lock();
        if let Some(state) = states.get_mut(&self.key) {
            state.in_flight = state.in_flight.saturating_sub(1);
        }
    }
}
