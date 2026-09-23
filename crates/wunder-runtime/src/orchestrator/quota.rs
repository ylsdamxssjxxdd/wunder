use super::*;
use crate::llm::LlmClient;

impl Orchestrator {
    pub(super) fn with_user_quota_admission(
        &self,
        client: LlmClient,
        user_id: &str,
        emitter: &EventEmitter,
        round_info: RoundInfo,
    ) -> LlmClient {
        let storage = self.storage.clone();
        let user_id = user_id.to_string();
        let emitter = emitter.clone();
        let monitor = self.monitor.clone();
        client.with_request_admission(move || {
            let storage = storage.clone();
            let user_id = user_id.clone();
            let emitter = emitter.clone();
            let monitor = monitor.clone();
            async move {
                let status = tokio::task::spawn_blocking(move || {
                    storage.consume_user_quota(
                        &user_id,
                        &UserStore::today_string(),
                        UserStore::default_daily_quota(),
                        1,
                    )
                })
                .await
                .map_err(|err| OrchestratorError::internal(err.to_string()))?
                .map_err(|err| OrchestratorError::internal(err.to_string()))?;
                let Some(status) = status else {
                    return Ok(());
                };
                if !status.allowed {
                    return Err(OrchestratorError::user_quota_insufficient(status).into());
                }
                // Hidden summaries and adapter fallbacks must update the same thread projection.
                let session_quota_used = monitor.record_quota_consumption(emitter.session_id(), 1);
                let mut payload = json!({
                    "consumed": 1,
                    "session_quota_used": session_quota_used,
                    "quota_balance": status.balance,
                    "quota_granted_total": status.granted_total,
                    "quota_used_total": status.used_total,
                    "daily_quota_grant": status.daily_grant,
                    "last_quota_grant_date": status.last_grant_date,
                    "remaining": status.balance,
                    "used": status.used_total,
                    "daily_quota": status.granted_total,
                    "date": status.last_grant_date,
                });
                round_info.insert_into(payload.as_object_mut().expect("quota object"));
                emitter.emit("quota_usage", payload).await;
                Ok(())
            }
        })
    }
}
