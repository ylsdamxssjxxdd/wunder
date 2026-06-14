use super::*;
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
            let status = tokio::task::spawn_blocking(move || {
                storage.try_acquire_session_lock(&session_id, &user_id, &agent_id, ttl, max_active)
            })
            .await
            .map_err(|err| anyhow!("session lock join error: {err}"))??;
            return Ok(matches!(status, SessionLockStatus::Acquired));
        }
        let retry_window = SESSION_LOCK_BUSY_RETRY_S.max(self.poll_interval_s);
        let retry_deadline = Instant::now() + Duration::from_secs_f64(retry_window);
        loop {
            let storage = self.storage.clone();
            let session_id = cleaned_session.to_string();
            let user_id = cleaned_user.to_string();
            let agent_id = cleaned_agent.to_string();
            let ttl = self.lock_ttl_s;
            let max_active = self.max_active;
            let status = tokio::task::spawn_blocking(move || {
                storage.try_acquire_session_lock(&session_id, &user_id, &agent_id, ttl, max_active)
            })
            .await
            .map_err(|err| anyhow!("session lock join error: {err}"))??;
            match status {
                SessionLockStatus::Acquired => return Ok(true),
                SessionLockStatus::UserBusy => {
                    if Instant::now() >= retry_deadline {
                        return Ok(false);
                    }
                    tokio::time::sleep(Duration::from_secs_f64(self.poll_interval_s)).await;
                }
                SessionLockStatus::SystemBusy => {
                    tokio::time::sleep(Duration::from_secs_f64(self.poll_interval_s)).await;
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
        match tokio::task::spawn_blocking(move || storage.touch_session_lock(&session_id, ttl))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!("failed to touch session lock for {cleaned_session}: {err}");
            }
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
        match tokio::task::spawn_blocking(move || storage.release_session_lock(&session_id)).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!("failed to release session lock for {cleaned_session}: {err}");
            }
            Err(err) => {
                warn!("failed to release session lock for {cleaned_session}: {err}");
            }
        }
    }
}
