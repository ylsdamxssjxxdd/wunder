//! Deterministic transient-fault injection for the synthetic simulator.
//!
//! Faults only apply to the random-simulation path (a turn with no recorded
//! log); recorded replay must stay byte-faithful to what was captured. Every
//! decision is derived from (session, user_round, model_round, attempt), so
//! cancel/resume and retries stay stable across restarts.

use super::random_sim::{mix, next_u64};
use super::{VirtualReplayTurn, RANDOM_REPLAY_LOG_ID};
use wunder_core::virtual_model::VirtualModelOptions;

/// A transient failure a real provider could return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulatedFault {
    /// Provider rejects the request before any output (5xx / rate limit).
    ApiError { code: &'static str },
    /// Connection drops mid-stream after emitting part of the answer.
    Disconnect,
}

const RATE_SCALE: u64 = 1_000_000;
const ERROR_CODES: [&str; 3] = ["server_error", "rate_limit_exceeded", "overloaded"];

/// True when the turn came from the synthetic simulator (no recorded log);
/// recorded replay is never faulted.
pub fn is_simulated_turn(turn: &VirtualReplayTurn) -> bool {
    turn.source_log_id == RANDOM_REPLAY_LOG_ID
}

/// Sample a transient fault for one attempt, or `None` for a clean attempt.
pub fn sample_fault(
    session_seed: &str,
    user_round: usize,
    model_round: usize,
    attempt: u32,
    options: &VirtualModelOptions,
) -> Option<SimulatedFault> {
    let error_rate = options.error_rate.clamp(0.0, 1.0);
    let disconnect_rate = options.disconnect_rate.clamp(0.0, 1.0);
    if error_rate <= 0.0 && disconnect_rate <= 0.0 {
        return None;
    }
    let mut state = mix(session_seed, fault_nonce(user_round, model_round, attempt, 0x11)).max(1);
    if roll(&mut state) < error_rate {
        let code = ERROR_CODES[(next_u64(&mut state) as usize) % ERROR_CODES.len()];
        return Some(SimulatedFault::ApiError { code });
    }
    if roll(&mut state) < disconnect_rate {
        return Some(SimulatedFault::Disconnect);
    }
    None
}

/// How many stream deltas to emit before dropping, so the client observes a
/// partial answer instead of an empty stream. Range: 1..=3.
pub fn disconnect_delta_limit(
    session_seed: &str,
    user_round: usize,
    model_round: usize,
    attempt: u32,
) -> usize {
    let mut state = mix(session_seed, fault_nonce(user_round, model_round, attempt, 0x22)).max(1);
    1 + (next_u64(&mut state) % 3) as usize
}

fn roll(state: &mut u64) -> f64 {
    (next_u64(state) % RATE_SCALE) as f64 / RATE_SCALE as f64
}

fn fault_nonce(user_round: usize, model_round: usize, attempt: u32, salt: u64) -> u64 {
    ((user_round as u64) << 40)
        ^ ((model_round as u64) << 16)
        ^ (attempt as u64)
        ^ salt.rotate_left(40)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(error_rate: f64, disconnect_rate: f64) -> VirtualModelOptions {
        VirtualModelOptions {
            error_rate,
            disconnect_rate,
            ..Default::default()
        }
    }

    #[test]
    fn zero_rates_never_fault() {
        let opts = options(0.0, 0.0);
        for round in 1..40 {
            for attempt in 1..4 {
                assert!(sample_fault("session-a", round, round, attempt, &opts).is_none());
            }
        }
    }

    #[test]
    fn certain_error_rate_always_faults() {
        let opts = options(1.0, 0.0);
        let fault = sample_fault("session-a", 1, 1, 1, &opts).expect("fault");
        assert!(matches!(fault, SimulatedFault::ApiError { .. }));
    }

    #[test]
    fn fault_sampling_is_deterministic() {
        let opts = options(0.5, 0.5);
        for round in 1..30 {
            let a = sample_fault("session-a", round, round, 1, &opts);
            let b = sample_fault("session-a", round, round, 1, &opts);
            assert_eq!(a, b);
        }
    }

    #[test]
    fn tiny_rates_fault_only_rarely() {
        let opts = options(0.01, 0.005);
        let mut faults = 0;
        for round in 1..=2000 {
            if sample_fault("session-a", round, 1, 1, &opts).is_some() {
                faults += 1;
            }
        }
        // Expect roughly 1.5% of 2000 ~= 30; generous bounds, fully deterministic.
        assert!(faults > 0 && faults < 150, "unexpected fault count: {faults}");
    }

    #[test]
    fn disconnect_limit_is_small_and_bounded() {
        for round in 1..20 {
            let limit = disconnect_delta_limit("session-a", round, 1, 1);
            assert!((1..=3).contains(&limit));
        }
    }
}
