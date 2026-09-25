use super::*;
use crate::llm::LlmClient;

impl Orchestrator {
    pub(super) fn with_quota_tracking_admission(
        &self,
        client: LlmClient,
        user_id: &str,
        is_admin: bool,
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
                let status = if is_admin {
                    None
                } else {
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
                    Some(status)
                };

                // Administrators bypass account debiting, but their model requests still
                // contribute to the thread-level usage projection. This keeps the user
                // bubble, thread list, and log overview consistent for every real request.
                let session_quota_used = monitor.record_quota_consumption(emitter.session_id(), 1);
                let turn_quota_used = emitter.record_quota_consumption(1);
                let mut payload = json!({
                    "consumed": 1,
                    "billable": !is_admin,
                    "turn_quota_used": turn_quota_used,
                    "session_quota_used": session_quota_used,
                });
                if let Some(status) = status {
                    let fields = payload.as_object_mut().expect("quota object");
                    fields.insert("quota_balance".to_string(), json!(status.balance));
                    fields.insert(
                        "quota_granted_total".to_string(),
                        json!(status.granted_total),
                    );
                    fields.insert("quota_used_total".to_string(), json!(status.used_total));
                    fields.insert("daily_quota_grant".to_string(), json!(status.daily_grant));
                    fields.insert(
                        "last_quota_grant_date".to_string(),
                        json!(status.last_grant_date),
                    );
                    fields.insert("remaining".to_string(), json!(status.balance));
                    fields.insert("used".to_string(), json!(status.used_total));
                    fields.insert("daily_quota".to_string(), json!(status.granted_total));
                    fields.insert("date".to_string(), json!(status.last_grant_date));
                }
                round_info.insert_into(payload.as_object_mut().expect("quota object"));
                emitter.emit("quota_usage", payload).await;
                Ok(())
            }
        })
    }
}
