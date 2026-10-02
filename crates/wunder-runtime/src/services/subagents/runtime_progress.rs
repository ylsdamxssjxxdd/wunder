//! Bounded child progress, persisted with its run and projected into its parent.
use super::*;
use std::time::{Duration, Instant};

pub(crate) struct RuntimeProgress {
    pub storage: Arc<dyn StorageBackend>,
    pub monitor: Option<Arc<MonitorState>>,
    pub record: SessionRunRecord,
    metrics: Value,
    last_publish: Instant,
    dirty: bool,
}

impl RuntimeProgress {
    pub fn new(
        storage: Arc<dyn StorageBackend>,
        monitor: Option<Arc<MonitorState>>,
        record: SessionRunRecord,
    ) -> Self {
        Self {
            storage,
            monitor,
            record,
            metrics: json!({
                "tool_calls": 0, "model_request_count": 0, "account_credits_consumed": 0,
                "context_tokens": null, "latest_message": ""
            }),
            last_publish: Instant::now(),
            dirty: false,
        }
    }

    pub async fn observe(
        &mut self,
        orchestrator: &Orchestrator,
        event: &str,
        data: &Value,
    ) -> Result<()> {
        self.dirty |= fold_progress(&mut self.metrics, event, data);
        if self.dirty
            && (matches!(event, "tool_call" | "llm_output" | "final" | "error")
                || self.last_publish.elapsed() >= Duration::from_millis(500))
        {
            self.flush(orchestrator).await?;
        }
        Ok(())
    }

    pub async fn flush(&mut self, orchestrator: &Orchestrator) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let metadata = self.record.metadata.get_or_insert_with(|| json!({}));
        metadata["subagent_progress"] = self.metrics.clone();
        self.record.updated_time = Utc::now().timestamp_millis() as f64 / 1000.0;
        let storage = self.storage.clone();
        let record = self.record.clone();
        crate::core::blocking::run_db("subagents.progress", move || {
            let mut latest = storage
                .get_session_run(&record.run_id)?
                .unwrap_or(record.clone());
            // Control actions may already have interrupted this run. Progress
            // must preserve their lifecycle state and unrelated metadata.
            latest.metadata.get_or_insert_with(|| json!({}))["subagent_progress"] =
                record.metadata.as_ref().unwrap()["subagent_progress"].clone();
            latest.updated_time = latest.updated_time.max(record.updated_time);
            storage.upsert_session_run(&latest)
        })
        .await?;
        if let Some(parent) = self.record.parent_session_id.as_deref() {
            emit_child_runtime_update(
                self.storage.clone(),
                self.monitor.clone(),
                orchestrator,
                &self.record.user_id,
                parent,
                &self.record.session_id,
            )
            .await?;
        }
        self.last_publish = Instant::now();
        self.dirty = false;
        Ok(())
    }
}

fn fold_progress(metrics: &mut Value, event: &str, data: &Value) -> bool {
    if let Some(turn_id) = data.get("turn_id").and_then(Value::as_str) {
        metrics["child_turn_id"] = json!(turn_id);
    }
    match event {
        "llm_request" => {
            metrics["latest_message"] = json!("");
        }
        "tool_call" => {
            metrics["tool_calls"] = json!(metrics["tool_calls"].as_i64().unwrap_or(0) + 1);
        }
        "model_request_usage" => {
            metrics["model_request_count"] = json!(
                metrics["model_request_count"].as_i64().unwrap_or(0)
                    + data["request_count"].as_i64().unwrap_or(1).max(0)
            );
            // This value is the turn's real debit, including zero for exempt calls.
            if let Some(credits) = data["account_credits_consumed"].as_i64() {
                metrics["account_credits_consumed"] = json!(credits.max(0));
            }
        }
        "context_usage" => {
            if let Some(tokens) = data
                .get("context_occupancy_tokens")
                .or_else(|| data.get("context_tokens"))
                .and_then(Value::as_i64)
            {
                metrics["context_tokens"] = json!(tokens.max(0));
            }
            if let Some(limit) = data["max_context"].as_i64().filter(|limit| *limit > 0) {
                metrics["max_context"] = json!(limit);
            }
        }
        "llm_output" | "final" => {
            if let Some(text) = data
                .get("answer")
                .or_else(|| data.get("content"))
                .and_then(Value::as_str)
            {
                metrics["latest_message"] =
                    json!(truncate_text(text, AUTO_WAKE_OBSERVATION_MAX_CHARS));
            }
        }
        "thread_item_delta" | "llm_output_delta" => {
            if event == "thread_item_delta" && data["source_event"] != "llm_output_delta" {
                return false;
            }
            if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                let mut preview = metrics["latest_message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let remaining =
                    AUTO_WAKE_OBSERVATION_MAX_CHARS.saturating_sub(preview.chars().count());
                if remaining == 0 {
                    return false;
                }
                preview.extend(delta.chars().take(remaining));
                metrics["latest_message"] = json!(preview);
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_keeps_real_debit_separate_and_bounds_message() {
        let mut metrics = json!({});
        fold_progress(
            &mut metrics,
            "model_request_usage",
            &json!({"request_count": 1, "account_credits_consumed": 0}),
        );
        fold_progress(&mut metrics, "tool_call", &json!({}));
        fold_progress(
            &mut metrics,
            "context_usage",
            &json!({"context_tokens": 2048}),
        );
        fold_progress(
            &mut metrics,
            "llm_output",
            &json!({"content": "文".repeat(1000)}),
        );
        assert_eq!(metrics["model_request_count"], 1);
        assert_eq!(metrics["account_credits_consumed"], 0);
        assert_eq!(metrics["tool_calls"], 1);
        assert_eq!(metrics["context_tokens"], 2048);
        assert!(metrics["latest_message"].as_str().unwrap().chars().count() <= 243);
        fold_progress(&mut metrics, "llm_request", &json!({}));
        fold_progress(
            &mut metrics,
            "thread_item_delta",
            &json!({"source_event": "llm_output_delta", "delta": "New reply"}),
        );
        assert_eq!(metrics["latest_message"], "New reply");
    }
}
