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
                let account = if is_admin {
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

                // A dispatched provider request is always observable, even when it is exempt
                // from account billing. Keeping request count and account debit separate makes
                // administrator sessions, retries, and compaction requests unambiguous.
                let session_model_requests = monitor.record_model_request(emitter.session_id(), 1);
                let turn_model_requests = emitter.record_model_request(1);
                let account_credits_consumed = if account.is_some() {
                    emitter.record_account_credit_consumption(1)
                } else {
                    emitter.accumulated_account_credit_consumption()
                };
                let mut payload = json!({
                    "request_count": 1,
                    "turn_request_count": turn_model_requests,
                    "session_request_count": session_model_requests,
                    "billable": !is_admin,
                    "account_credits_consumed": account_credits_consumed,
                });
                if let Some(status) = account {
                    let fields = payload.as_object_mut().expect("model request usage object");
                    fields.insert(
                        "account".to_string(),
                        json!({
                            "balance": status.balance,
                            "granted_total": status.granted_total,
                            "used_total": status.used_total,
                            "daily_grant": status.daily_grant,
                            "last_grant_date": status.last_grant_date,
                        }),
                    );
                }
                round_info
                    .insert_into(payload.as_object_mut().expect("model request usage object"));
                emitter.emit("model_request_usage", payload).await;
                Ok(())
            }
        })
    }
}
