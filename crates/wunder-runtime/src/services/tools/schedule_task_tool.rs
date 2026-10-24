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
    let args = super::recover_tool_args_value(args);
    validate_schedule_tool_args(&args)?;
    let normalized = normalize_cron_action_args(&args);
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

/// Validate the compact model-facing schedule surface before deserializing it
/// into the wider cron service request. This keeps action-specific fields from
/// silently crossing actions and turns typos into an immediate, actionable
/// tool error instead of a partially applied job patch.
fn validate_schedule_tool_args(args: &Value) -> Result<()> {
    let object = args
        .as_object()
        .ok_or_else(|| anyhow!("schedule_task arguments must be a JSON object"))?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("schedule_task action is required"))?;
    let allowed = match action {
        "add" => &[
            "action",
            "job",
            "job_id",
            "name",
            "schedule",
            "schedule_text",
            "session",
            "message",
            "payload",
            "deliver",
            "enabled",
            "delete_after_run",
            "dedupe_key",
            "session_id",
            "agent_id",
        ][..],
        "update" => &[
            "action",
            "job",
            "job_id",
            "name",
            "schedule",
            "schedule_text",
            "session",
            "message",
            "payload",
            "deliver",
            "enabled",
            "delete_after_run",
            "dedupe_key",
            "session_id",
            "agent_id",
        ][..],
        "get" | "remove" | "enable" | "disable" | "run" => &["action", "job", "job_id"][..],
        "list" | "status" => &["action"][..],
        _ => return Err(anyhow!("unsupported schedule action: {action}")),
    };
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(anyhow!(
            "schedule_task field '{key}' is not valid for action '{action}'"
        ));
    }
    if matches!(action, "get" | "remove" | "enable" | "disable" | "run")
        && !object
            .get("job_id")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        && !object
            .get("job")
            .and_then(|job| job.get("job_id"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Err(anyhow!("schedule_task {action} requires job_id"));
    }
    if let Some(job) = object.get("job") {
        if !job.is_object() {
            return Err(anyhow!("schedule_task job must be an object"));
        }
        // The nested shape is an internal/API compatibility carrier. The
        // model-facing schema is flat, but reject unknown nested fields here
        // so the carrier cannot bypass the action boundary.
        let allowed_job = [
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
        ];
        let job_object = job.as_object().expect("object checked above");
        if let Some(key) = job_object
            .keys()
            .find(|key| !allowed_job.contains(&key.as_str()))
        {
            return Err(anyhow!("schedule_task job field '{key}' is not recognized"));
        }
    }
    if matches!(action, "add" | "update") {
        let has_message = object
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        let has_payload_message = object
            .get("payload")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        let has_nested_payload = object
            .get("job")
            .and_then(|job| job.get("payload"))
            .and_then(|payload| payload.get("message"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if action == "add" && !(has_message || has_payload_message || has_nested_payload) {
            return Err(anyhow!(
                "schedule_task add requires message or payload.message"
            ));
        }
        if action == "add"
            && object.get("schedule").is_none()
            && object
                .get("schedule_text")
                .and_then(Value::as_str)
                .is_none()
            && object
                .get("job")
                .and_then(|job| job.get("schedule").or_else(|| job.get("schedule_text")))
                .is_none()
        {
            return Err(anyhow!(
                "schedule_task add requires schedule or schedule_text"
            ));
        }
    }
    Ok(())
}

/// Deserialize only the two scheduling booleans from unambiguous JSON literals.
/// Some model providers transport `false` as the string `"False"`; accepting
/// that literal here prevents a recoverable formatting artifact from blocking
/// a harmless disabled job, while all other strings remain untouched.
fn normalize_schedule_boolean_literals(value: &Value) -> Value {
    fn normalize(value: &Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.iter()
                    .map(|(key, value)| {
                        let value = if matches!(key.as_str(), "enabled" | "delete_after_run") {
                            value
                                .as_str()
                                .and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                                    "true" => Some(Value::Bool(true)),
                                    "false" => Some(Value::Bool(false)),
                                    _ => None,
                                })
                                .unwrap_or_else(|| normalize(value))
                        } else {
                            normalize(value)
                        };
                        (key.clone(), value)
                    })
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(normalize).collect()),
            _ => value.clone(),
        }
    }
    normalize(value)
}

fn normalize_cron_action_args(args: &Value) -> Value {
    let normalized_booleans = normalize_schedule_boolean_literals(args);
    let Some(obj) = normalized_booleans.as_object() else {
        return normalized_booleans;
    };
    if obj.contains_key("job") {
        return normalized_booleans;
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
    for key in [
        "running",
        "resolved_session_id",
        "run_session_id",
        "delivery_session_id",
        "last_status",
        "last_error",
    ] {
        if let Some(value) = value.get(key) {
            output[key] = value.clone();
        }
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
        "running": job.get("running").cloned().unwrap_or(Value::Null),
        "schedule": schedule,
        "next_run_at": next_run,
        "last_run_at": last_run,
        "last_status": job.get("last_status").cloned().unwrap_or(Value::Null),
        "last_error": job.get("last_error").cloned().unwrap_or(Value::Null),
        "session_target": job.get("session_target").cloned().unwrap_or(Value::Null),
        "session_id": job.get("session_id").cloned().unwrap_or(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_cron_action_args;
    use serde_json::json;

    #[test]
    fn cron_result_keeps_routing_running_and_failure_diagnostics() {
        let result = super::compact_cron_tool_result(json!({"action":"get", "job":{
            "session_id":"fixture-thread", "session_target":"isolated", "running":true,
            "last_status":"error", "last_error":"fixture failure"}}));
        assert_eq!(result["job"]["session_id"], "fixture-thread");
        assert_eq!(result["job"]["running"], true);
        assert_eq!(result["job"]["last_error"], "fixture failure");
    }

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
    fn normalize_cron_action_args_recovers_boolean_literals() {
        let normalized = normalize_cron_action_args(&json!({
            "action": "add",
            "message": "fixture",
            "schedule_text": "every 5 minutes",
            "enabled": "False",
            "delete_after_run": "TRUE"
        }));
        assert_eq!(normalized["job"]["enabled"], json!(false));
        assert_eq!(normalized["job"]["delete_after_run"], json!(true));
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

    #[test]
    fn validate_schedule_actions_reject_cross_action_fields() {
        assert!(
            super::validate_schedule_tool_args(&json!({"action":"list","job_id":"x"})).is_err()
        );
        assert!(super::validate_schedule_tool_args(&json!({"action":"get"})).is_err());
        assert!(super::validate_schedule_tool_args(
            &json!({"action":"add","message":"m","schedule_text":"every 1 minute"})
        )
        .is_ok());
    }
}
