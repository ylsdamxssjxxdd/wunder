use super::*;
use crate::core::blocking;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct RequestLimiter {
    storage: Arc<dyn StorageBackend>,
    max_active: i64,
    poll_interval_s: f64,
    lock_ttl_s: f64,
}

impl RequestLimiter {
    pub(super) fn new(storage: Arc<dyn StorageBackend>, max_active: usize) -> Self {
        Self {
            storage,
            max_active: max_active.max(1) as i64,
            poll_interval_s: SESSION_LOCK_POLL_INTERVAL_S,
            lock_ttl_s: SESSION_LOCK_TTL_S,
        }
    }

    pub(super) async fn acquire(
        &self,
        session_id: &str,
        user_id: &str,
        agent_id: &str,
        allow_queue: bool,
    ) -> Result<bool> {
        let cleaned_session = session_id.trim();
        let cleaned_user = user_id.trim();
        let cleaned_agent = agent_id.trim();
        if cleaned_session.is_empty() || cleaned_user.is_empty() {
            return Ok(false);
        }
        if !allow_queue {
            let storage = self.storage.clone();
            let session_id = cleaned_session.to_string();
            let user_id = cleaned_user.to_string();
            let agent_id = cleaned_agent.to_string();
            let ttl = self.lock_ttl_s;
            let max_active = self.max_active;
            let status = blocking::run_db("orchestrator.limiter.acquire_nowait", move || {
                storage.try_acquire_session_lock(&session_id, &user_id, &agent_id, ttl, max_active)
            })
            .await
            .map_err(|err| anyhow!("session lock acquire error: {err}"))?;
            return Ok(matches!(status, SessionLockStatus::Acquired));
        }
        let retry_window = SESSION_LOCK_BUSY_RETRY_S.max(self.poll_interval_s);
        let user_deadline = Instant::now() + Duration::from_secs_f64(retry_window);
        let wait_started = Instant::now();
        let busy_phase = waiter_spread_s(cleaned_session);
        let mut system_attempts = 0u32;
        let mut warned = false;
        loop {
            let storage = self.storage.clone();
            let session_id = cleaned_session.to_string();
            let user_id = cleaned_user.to_string();
            let agent_id = cleaned_agent.to_string();
            let ttl = self.lock_ttl_s;
            let max_active = self.max_active;
            let status = blocking::run_db("orchestrator.limiter.acquire", move || {
                storage.try_acquire_session_lock(&session_id, &user_id, &agent_id, ttl, max_active)
            })
            .await
            .map_err(|err| anyhow!("session lock acquire error: {err}"))?;
            match status {
                SessionLockStatus::Acquired => return Ok(true),
                SessionLockStatus::UserBusy => {
                    if Instant::now() >= user_deadline {
                        return Ok(false);
                    }
                    tokio::time::sleep(Duration::from_secs_f64(self.poll_interval_s)).await;
                }
                SessionLockStatus::SystemBusy => {
                    // A global-slot wait keeps waiting: USER_BUSY costs the durable
                    // queue one of its two retry attempts, so failing here would
                    // retire tasks that are only behind a busy system.
                    system_attempts = system_attempts.saturating_add(1);
                    if !warned {
                        let waited_s = wait_started.elapsed().as_secs_f64();
                        if waited_s >= SESSION_LOCK_BUSY_WARN_S {
                            warned = true;
                            warn!(
                                "session lock for {cleaned_session} is still waiting on global \
                                 capacity after {waited_s:.0}s (attempt {system_attempts})"
                            );
                        }
                    }
                    let wait_s =
                        system_busy_wait_s(self.poll_interval_s, system_attempts, busy_phase);
                    tokio::time::sleep(Duration::from_secs_f64(wait_s)).await;
                }
            }
        }
    }

    pub(super) async fn touch(&self, session_id: &str) {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return;
        }
        let storage = self.storage.clone();
        let session_id = cleaned_session.to_string();
        let ttl = self.lock_ttl_s;
        match blocking::run_db("orchestrator.limiter.touch", move || {
            storage.touch_session_lock(&session_id, ttl)
        })
        .await
        {
            Ok(()) => {}
            Err(err) => {
                warn!("failed to touch session lock for {cleaned_session}: {err}");
            }
        }
    }

    pub(super) async fn release(&self, session_id: &str) {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return;
        }
        let storage = self.storage.clone();
        let session_id = cleaned_session.to_string();
        match blocking::run_db("orchestrator.limiter.release", move || {
            storage.release_session_lock(&session_id)
        })
        .await
        {
            Ok(()) => {}
            Err(err) => {
                warn!("failed to release session lock for {cleaned_session}: {err}");
            }
        }
    }
}

/// Wait length for a global-slot wait. Every attempt takes a DB permit, so the
/// cadence grows instead of staying on the fixed poll interval.
fn system_busy_wait_s(base_s: f64, attempts: u32, phase_s: f64) -> f64 {
    let steps = attempts
        .saturating_sub(SESSION_LOCK_BUSY_BACKOFF_AFTER)
        .min(8) as f64;
    let grown = base_s * SESSION_LOCK_BUSY_BACKOFF_FACTOR.powf(steps);
    grown.min(SESSION_LOCK_BUSY_MAX_INTERVAL_S) + phase_s
}

/// Stable per-session offset inside the spread band so a full system does not
/// wake every waiter on the same tick.
fn waiter_spread_s(session_id: &str) -> f64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in session_id.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % 1_000) as f64 / 1_000.0 * SESSION_LOCK_BUSY_MAX_SPREAD_S
}

#[cfg(test)]
mod tests {
    use super::{system_busy_wait_s, waiter_spread_s};
    use super::{
        SESSION_LOCK_BUSY_MAX_INTERVAL_S, SESSION_LOCK_BUSY_MAX_SPREAD_S,
        SESSION_LOCK_POLL_INTERVAL_S,
    };

    #[test]
    fn system_busy_wait_starts_at_poll_interval_then_grows_and_caps() {
        let phase = 0.0;
        assert_eq!(
            system_busy_wait_s(SESSION_LOCK_POLL_INTERVAL_S, 1, phase),
            SESSION_LOCK_POLL_INTERVAL_S
        );
        let first = system_busy_wait_s(SESSION_LOCK_POLL_INTERVAL_S, 3, phase);
        let second = system_busy_wait_s(SESSION_LOCK_POLL_INTERVAL_S, 4, phase);
        assert!(
            second > first,
            "global-slot waits must back off: {first} -> {second}"
        );
        let capped = system_busy_wait_s(SESSION_LOCK_POLL_INTERVAL_S, 40, phase);
        assert_eq!(capped, SESSION_LOCK_BUSY_MAX_INTERVAL_S);
    }

    #[test]
    fn system_busy_wait_stays_within_capped_spread_band() {
        for attempts in 1..=60u32 {
            let wait = system_busy_wait_s(SESSION_LOCK_POLL_INTERVAL_S, attempts, 0.5);
            assert!(wait <= SESSION_LOCK_BUSY_MAX_INTERVAL_S + SESSION_LOCK_BUSY_MAX_SPREAD_S);
            assert!(wait > 0.0);
        }
    }

    #[test]
    fn waiter_spread_is_stable_bounded_and_splits_waiters() {
        let again = waiter_spread_s("session_fixed");
        assert_eq!(again, waiter_spread_s("session_fixed"));
        assert!((0.0..SESSION_LOCK_BUSY_MAX_SPREAD_S).contains(&again));
        let distinct: std::collections::HashSet<i64> = (0..64)
            .map(|index| (waiter_spread_s(&format!("session_{index}")) * 1_000.0) as i64)
            .collect();
        assert!(
            distinct.len() > 8,
            "spread must stagger waiters, got {} buckets",
            distinct.len()
        );
    }
}
