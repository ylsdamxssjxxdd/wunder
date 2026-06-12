use super::context::ToolContext;
use crate::cron::{handle_cron_action, CronActionRequest};
use crate::i18n;
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) async fn execute_schedule_task_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let normalized = normalize_cron_action_args(args);
    let payload: CronActionRequest = serde_json::from_value(normalized.clone()).map_err(|err| {
        if normalized.get("raw").is_some() {
            anyhow!(
                "schedule_task arguments are not valid JSON; retry with a complete JSON object including action"
            )
        } else {
            anyhow!(err.to_string())
        }
    })?;
    let user_tool_manager = context
        .user_tool_manager
        .clone()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let user_store = Arc::new(UserStore::new(context.storage.clone()));
    let skills = Arc::new(RwLock::new(context.skills.clone()));
    handle_cron_action(
        context.config.clone(),
        context.storage.clone(),
        context.orchestrator.clone(),
        context.cron_wake_signal.clone(),
        user_store,
        user_tool_manager,
        skills,
        context.user_id,
        Some(context.session_id),
        context.agent_id,
        payload,
    )
    .await
    .map(compact_cron_tool_result)
}

fn normalize_cron_action_args(args: &Value) -> Value {
    let Some(obj) = args.as_object() else {
        return args.clone();
    };
    if obj.contains_key("job") {
        return args.clone();
    }

    let mut normalized = obj.clone();
    let mut job = Map::new();
    for key in [
        "job_id",
        "name",
        "schedule",
        "schedule_text",
        "session",
        "payload",
        "deliver",
        "enabled",
        "delete_after_run",
        "dedupe_key",
        "session_id",
        "agent_id",
    ] {
        if let Some(value) = normalized.remove(key) {
            job.insert(key.to_string(), value);
        }
    }

    if let Some(message) = normalized.remove("message") {
        match job.get_mut("payload") {
            Some(Value::Object(payload)) => {
                payload.entry("message".to_string()).or_insert(message);
            }
            _ => {
                let mut payload = Map::new();
                payload.insert("message".to_string(), message);
                job.insert("payload".to_string(), Value::Object(payload));
            }
        }
    }

    if !job.is_empty() {
        normalized.insert("job".to_string(), Value::Object(job));
    }
    Value::Object(normalized)
}

fn compact_cron_tool_result(value: Value) -> Value {
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut output = json!({ "action": action });
    if let Some(removed) = value.get("removed") {
        output["removed"] = removed.clone();
    }
    if let Some(queued) = value.get("queued") {
        output["queued"] = queued.clone();
    }
    if let Some(reason) = value.get("reason") {
        output["reason"] = reason.clone();
    }
    if let Some(deduped) = value.get("deduped") {
        output["deduped"] = deduped.clone();
    }
    if let Some(job) = value.get("job") {
        output["job"] = compact_cron_job(job);
    }
    if let Some(jobs) = value.get("jobs") {
        output["jobs"] = compact_cron_jobs(jobs);
    }
    if let Some(scheduler) = value.get("scheduler") {
        output["scheduler"] = scheduler.clone();
    }
    if let Some(user_jobs) = value.get("user_jobs") {
        output["user_jobs"] = user_jobs.clone();
    }
    output
}

fn compact_cron_jobs(value: &Value) -> Value {
    let Some(items) = value.as_array() else {
        return Value::Array(Vec::new());
    };
    let jobs = items.iter().map(compact_cron_job).collect::<Vec<_>>();
    Value::Array(jobs)
}

fn compact_cron_job(job: &Value) -> Value {
    let schedule = job.get("schedule").and_then(Value::as_object);
    let schedule = json!({
        "kind": schedule.and_then(|map| map.get("kind")).cloned().unwrap_or(Value::Null),
        "at": schedule.and_then(|map| map.get("at")).cloned().unwrap_or(Value::Null),
        "every_ms": schedule.and_then(|map| map.get("every_ms")).cloned().unwrap_or(Value::Null),
        "cron": schedule.and_then(|map| map.get("cron")).cloned().unwrap_or(Value::Null),
        "tz": schedule.and_then(|map| map.get("tz")).cloned().unwrap_or(Value::Null)
    });
    let next_run = job
        .get("next_run_at_text")
        .cloned()
        .or_else(|| job.get("next_run_at").cloned())
        .unwrap_or(Value::Null);
    let last_run = job
        .get("last_run_at_text")
        .cloned()
        .or_else(|| job.get("last_run_at").cloned())
        .unwrap_or(Value::Null);
    json!({
        "job_id": job.get("job_id").cloned().unwrap_or(Value::Null),
        "name": job.get("name").cloned().unwrap_or(Value::Null),
        "enabled": job.get("enabled").cloned().unwrap_or(Value::Null),
        "schedule": schedule,
        "next_run_at": next_run,
        "last_run_at": last_run,
        "last_status": job.get("last_status").cloned().unwrap_or(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_cron_action_args;
    use serde_json::json;

    #[test]
    fn normalize_cron_action_args_flattens_message_into_job_payload() {
        let normalized = normalize_cron_action_args(&json!({
            "action": "add",
            "job_id": "job_demo",
            "schedule_text": "every 5 minutes",
            "message": "hello",
            "enabled": true
        }));
        assert_eq!(normalized["action"], json!("add"));
        assert_eq!(normalized["job"]["job_id"], json!("job_demo"));
        assert_eq!(normalized["job"]["schedule_text"], json!("every 5 minutes"));
        assert_eq!(normalized["job"]["payload"]["message"], json!("hello"));
        assert_eq!(normalized["job"]["enabled"], json!(true));
        assert!(normalized.get("message").is_none());
    }

    #[test]
    fn normalize_cron_action_args_preserves_existing_job_object() {
        let original = json!({
            "action": "add",
            "job": {
                "job_id": "job_demo",
                "payload": { "message": "hello" }
            }
        });
        let normalized = normalize_cron_action_args(&original);
        assert_eq!(normalized, original);
    }
}
