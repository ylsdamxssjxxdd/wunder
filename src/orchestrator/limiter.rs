use super::*;

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
    ) -> Result<bool> {
        if session_id.trim().is_empty() || user_id.trim().is_empty() {
            return Ok(false);
        }
        loop {
            let storage = self.storage.clone();
            let session_id = session_id.to_string();
            let user_id = user_id.to_string();
            let agent_id = agent_id.to_string();
            let ttl = self.lock_ttl_s;
            let max_active = self.max_active;
            let status = tokio::task::spawn_blocking(move || {
                storage.try_acquire_session_lock(&session_id, &user_id, &agent_id, ttl, max_active)
            })
            .await
            .map_err(|err| anyhow!("session lock join error: {err}"))??;
            match status {
                SessionLockStatus::Acquired => return Ok(true),
                SessionLockStatus::UserBusy => return Ok(false),
                SessionLockStatus::SystemBusy => {
                    tokio::time::sleep(std::time::Duration::from_secs_f64(self.poll_interval_s))
                        .await;
                }
            }
        }
    }

    pub(super) async fn touch(&self, session_id: &str) {
        let storage = self.storage.clone();
        let session_id = session_id.to_string();
        let ttl = self.lock_ttl_s;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = storage.touch_session_lock(&session_id, ttl);
            });
        } else {
            let _ = storage.touch_session_lock(&session_id, ttl);
        }
    }

    pub(super) async fn release(&self, session_id: &str) {
        let storage = self.storage.clone();
        let session_id = session_id.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || {
                let _ = storage.release_session_lock(&session_id);
            });
        } else {
            let _ = storage.release_session_lock(&session_id);
        }
    }
}
