use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::i18n;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::skills::SkillRegistry;
use crate::storage::{
    ChatSessionRecord, CronJobRecord, CronRunRecord, StorageBackend, UserAccountRecord,
    UserAgentRecord,
};
use crate::user_access::{compute_allowed_tool_names, is_agent_allowed, UserToolContext};
use crate::user_store::UserStore;
use crate::user_tools::UserToolManager;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_stream::StreamExt;
use tracing::error;
use uuid::Uuid;

const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "新会话";
const SUMMARY_MAX_CHARS: usize = 200;

#[derive(Debug, Deserialize, Clone)]
pub struct CronActionRequest {
    pub action: String,
    #[serde(default)]
    pub job: Option<CronJobInput>,
}

pub async fn handle_cron_action(
    config: Config,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Option<Arc<Orchestrator>>,
    user_store: Arc<UserStore>,
    user_tool_manager: Arc<UserToolManager>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_id: &str,
    session_id: Option<&str>,
    agent_id: Option<&str>,
    payload: CronActionRequest,
) -> Result<Value> {
    let action = payload.action.trim().to_lowercase();
    let now = now_ts();
    match action.as_str() {
        "list" => {
            let storage = storage.clone();
            let cleaned = user_id.trim().to_string();
            let jobs = tokio::task::spawn_blocking(move || storage.list_cron_jobs(&cleaned, true))
                .await
                .map_err(|err| anyhow!(err.to_string()))??;
            let items = jobs.iter().map(cron_job_to_value).collect::<Vec<_>>();
            Ok(json!({ "action": "list", "jobs": items }))
        }
        "get" => {
            let job_id = payload
                .job
                .as_ref()
                .and_then(|job| job.job_id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("job_id required"))?;
            let storage = storage.clone();
            let cleaned_user = user_id.trim().to_string();
            let cleaned_job = job_id.to_string();
            let job = tokio::task::spawn_blocking(move || {
                storage.get_cron_job(&cleaned_user, &cleaned_job)
            })
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
            let Some(job) = job else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            Ok(json!({ "action": "get", "job": cron_job_to_value(&job) }))
        }
        "add" => {
            let input = payload.job.ok_or_else(|| anyhow!("job required"))?;
            let job_session_id = input
                .session_id
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .or_else(|| session_id.map(|value| value.to_string()))
                .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
            let dedupe_key = input
                .dedupe_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string());
            if let Some(key) = dedupe_key.as_ref() {
                let storage = storage.clone();
                let cleaned_user = user_id.trim().to_string();
                let key = key.clone();
                let existing = {
                    let storage = storage.clone();
                    tokio::task::spawn_blocking(move || {
                        storage.get_cron_job_by_dedupe_key(&cleaned_user, &key)
                    })
                    .await
                    .map_err(|err| anyhow!(err.to_string()))??
                };
                if let Some(mut record) = existing {
                    apply_job_patch(
                        &mut record,
                        &input,
                        job_session_id.as_str(),
                        agent_id,
                        now,
                        true,
                    )?;
                    let storage = storage.clone();
                    let record_clone = record.clone();
                    tokio::task::spawn_blocking(move || storage.upsert_cron_job(&record_clone))
                        .await
                        .map_err(|err| anyhow!(err.to_string()))??;
                    return Ok(json!({
                        "action": "update",
                        "job": cron_job_to_value(&record),
                        "deduped": true
                    }));
                }
            }
            let record = build_job_record(user_id, job_session_id.as_str(), agent_id, now, input)?;
            let storage = storage.clone();
            let record_clone = record.clone();
            tokio::task::spawn_blocking(move || storage.upsert_cron_job(&record_clone))
                .await
                .map_err(|err| anyhow!(err.to_string()))??;
            Ok(json!({ "action": "add", "job": cron_job_to_value(&record) }))
        }
        "update" => {
            let input = payload.job.ok_or_else(|| anyhow!("job required"))?;
            let job_id = input
                .job_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("job_id required"))?;
            let cleaned_user = user_id.trim().to_string();
            let cleaned_job = job_id.to_string();
            let existing = {
                let storage = storage.clone();
                tokio::task::spawn_blocking(move || {
                    storage.get_cron_job(&cleaned_user, &cleaned_job)
                })
                .await
                .map_err(|err| anyhow!(err.to_string()))??
            };
            let Some(mut record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            let fallback_session_id = record.session_id.clone();
            let job_session_id = input
                .session_id
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or(fallback_session_id);
            apply_job_patch(
                &mut record,
                &input,
                job_session_id.as_str(),
                agent_id,
                now,
                false,
            )?;
            let storage = storage.clone();
            let record_clone = record.clone();
            tokio::task::spawn_blocking(move || storage.upsert_cron_job(&record_clone))
                .await
                .map_err(|err| anyhow!(err.to_string()))??;
            Ok(json!({ "action": "update", "job": cron_job_to_value(&record) }))
        }
        "enable" | "disable" => {
            let job_id = payload
                .job
                .as_ref()
                .and_then(|job| job.job_id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("job_id required"))?;
            let cleaned_user = user_id.trim().to_string();
            let cleaned_job = job_id.to_string();
            let existing = {
                let storage = storage.clone();
                tokio::task::spawn_blocking(move || {
                    storage.get_cron_job(&cleaned_user, &cleaned_job)
                })
                .await
                .map_err(|err| anyhow!(err.to_string()))??
            };
            let Some(mut record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            record.enabled = action == "enable";
            if record.enabled {
                record.next_run_at = compute_next_run_at(
                    &record.schedule_kind,
                    record.schedule_at.as_deref(),
                    record.schedule_every_ms,
                    record.schedule_cron.as_deref(),
                    record.schedule_tz.as_deref(),
                    record.created_at,
                    now,
                );
            } else {
                record.next_run_at = None;
                record.running_at = None;
            }
            record.updated_at = now;
            let storage = storage.clone();
            let record_clone = record.clone();
            tokio::task::spawn_blocking(move || storage.upsert_cron_job(&record_clone))
                .await
                .map_err(|err| anyhow!(err.to_string()))??;
            Ok(json!({ "action": action, "job": cron_job_to_value(&record) }))
        }
        "remove" => {
            let job_id = payload
                .job
                .as_ref()
                .and_then(|job| job.job_id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("job_id required"))?;
            let storage = storage.clone();
            let cleaned_user = user_id.trim().to_string();
            let cleaned_job = job_id.to_string();
            let removed = tokio::task::spawn_blocking(move || {
                storage.delete_cron_job(&cleaned_user, &cleaned_job)
            })
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
            Ok(json!({ "action": "remove", "removed": removed > 0 }))
        }
        "run" => {
            let job_id = payload
                .job
                .as_ref()
                .and_then(|job| job.job_id.as_deref())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("job_id required"))?;
            let cleaned_user = user_id.trim().to_string();
            let cleaned_job = job_id.to_string();
            let existing = {
                let storage = storage.clone();
                tokio::task::spawn_blocking(move || {
                    storage.get_cron_job(&cleaned_user, &cleaned_job)
                })
                .await
                .map_err(|err| anyhow!(err.to_string()))??
            };
            let Some(mut record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            if record.running_at.is_some() {
                return Ok(json!({
                    "action": "run",
                    "queued": false,
                    "reason": "running",
                    "job": cron_job_to_value(&record)
                }));
            }
            record.running_at = Some(now);
            record.updated_at = now;
            let record_for_upsert = record.clone();
            let record_for_response = record.clone();
            let storage_for_upsert = storage.clone();
            tokio::task::spawn_blocking(move || {
                storage_for_upsert.upsert_cron_job(&record_for_upsert)
            })
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
            let Some(orchestrator) = orchestrator else {
                return Ok(json!({
                    "action": "run",
                    "queued": true,
                    "job": cron_job_to_value(&record)
                }));
            };
            let runtime = CronRuntime::from_parts(
                config,
                storage.clone(),
                orchestrator,
                user_store,
                user_tool_manager,
                skills,
            );
            let handle = tokio::runtime::Handle::current();
            tokio::task::spawn_blocking(move || {
                handle.block_on(runtime.execute_job(record, "manual"));
            });
            Ok(
                json!({ "action": "run", "queued": true, "job": cron_job_to_value(&record_for_response) }),
            )
        }
        _ => Err(anyhow!("unsupported action: {}", payload.action)),
    }
}

pub async fn list_cron_runs(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    job_id: &str,
    limit: i64,
) -> Result<Value> {
    let cleaned_user = user_id.trim().to_string();
    let cleaned_job = job_id.trim().to_string();
    let safe_limit = limit.max(1).min(200);
    let storage = storage.clone();
    let runs = {
        let job_id = cleaned_job.clone();
        tokio::task::spawn_blocking(move || {
            storage.list_cron_runs(&cleaned_user, &job_id, safe_limit)
        })
        .await
        .map_err(|err| anyhow!(err.to_string()))??
    };
    let items = runs.iter().map(cron_run_to_value).collect::<Vec<_>>();
    Ok(json!({ "job_id": cleaned_job, "runs": items }))
}

fn build_job_record(
    user_id: &str,
    session_id: &str,
    agent_id: Option<&str>,
    now: f64,
    input: CronJobInput,
) -> Result<CronJobRecord> {
    let schedule = input
        .schedule
        .as_ref()
        .ok_or_else(|| anyhow!("schedule required"))?;
    let (schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz) =
        normalize_schedule_input(schedule)?;
    let payload = input.payload.unwrap_or(Value::Null);
    if extract_payload_message(Some(&payload)).is_none() {
        return Err(anyhow!("payload.message required"));
    }
    let enabled = input.enabled.unwrap_or(true);
    let delete_after_run = input.delete_after_run.unwrap_or(false);
    let session_target = normalize_session_target(input.session.as_deref());
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| extract_payload_message(Some(&payload)).map(|value| truncate_text(&value, 24)));
    let dedupe_key = input
        .dedupe_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());
    let job_id = input
        .job_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let next_run_at = if enabled {
        compute_next_run_at(
            &schedule_kind,
            schedule_at.as_deref(),
            schedule_every_ms,
            schedule_cron.as_deref(),
            schedule_tz.as_deref(),
            now,
            now,
        )
    } else {
        None
    };
    Ok(CronJobRecord {
        job_id,
        user_id: user_id.to_string(),
        session_id: session_id.to_string(),
        agent_id: agent_id.map(|value| value.to_string()),
        name,
        session_target,
        payload,
        deliver: input.deliver,
        enabled,
        delete_after_run,
        schedule_kind,
        schedule_at,
        schedule_every_ms,
        schedule_cron,
        schedule_tz,
        dedupe_key,
        next_run_at,
        running_at: None,
        last_run_at: None,
        last_status: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    })
}

fn apply_job_patch(
    record: &mut CronJobRecord,
    input: &CronJobInput,
    session_id: &str,
    agent_id: Option<&str>,
    now: f64,
    allow_missing_payload: bool,
) -> Result<()> {
    if let Some(name) = input.name.as_deref() {
        let trimmed = name.trim();
        record.name = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    record.session_id = session_id.to_string();
    if let Some(agent_id) = agent_id {
        record.agent_id = Some(agent_id.to_string());
    } else if input.agent_id.is_some() {
        record.agent_id = input
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());
    }
    if let Some(session_target) = input.session.as_deref() {
        record.session_target = normalize_session_target(Some(session_target));
    }
    if let Some(payload) = input.payload.as_ref() {
        if extract_payload_message(Some(payload)).is_none() && !allow_missing_payload {
            return Err(anyhow!("payload.message required"));
        }
        record.payload = payload.clone();
    } else if !allow_missing_payload && extract_payload_message(Some(&record.payload)).is_none() {
        return Err(anyhow!("payload.message required"));
    }
    if let Some(deliver) = input.deliver.as_ref() {
        record.deliver = Some(deliver.clone());
    }
    if let Some(enabled) = input.enabled {
        record.enabled = enabled;
    }
    if let Some(delete_after_run) = input.delete_after_run {
        record.delete_after_run = delete_after_run;
    }
    if let Some(dedupe_key) = input.dedupe_key.as_deref() {
        let trimmed = dedupe_key.trim();
        record.dedupe_key = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
    }
    if let Some(schedule) = input.schedule.as_ref() {
        let (kind, at, every, cron, tz) = normalize_schedule_input(schedule)?;
        record.schedule_kind = kind;
        record.schedule_at = at;
        record.schedule_every_ms = every;
        record.schedule_cron = cron;
        record.schedule_tz = tz;
    }
    record.updated_at = now;
    if record.enabled {
        record.next_run_at = compute_next_run_at(
            &record.schedule_kind,
            record.schedule_at.as_deref(),
            record.schedule_every_ms,
            record.schedule_cron.as_deref(),
            record.schedule_tz.as_deref(),
            record.created_at,
            now,
        );
    } else {
        record.next_run_at = None;
        record.running_at = None;
    }
    Ok(())
}

fn normalize_schedule_input(
    schedule: &CronScheduleInput,
) -> Result<(
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
)> {
    let kind = schedule.kind.trim().to_lowercase();
    match kind.as_str() {
        "at" => {
            let at = schedule
                .at
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("schedule.at required"))?;
            if parse_rfc3339(at).is_none() {
                return Err(anyhow!("invalid schedule.at"));
            }
            Ok((kind, Some(at.to_string()), None, None, None))
        }
        "every" => {
            let every_ms = schedule.every_ms.unwrap_or(0).max(1000);
            Ok((kind, None, Some(every_ms), None, None))
        }
        "cron" => {
            let expr = schedule
                .cron
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("schedule.cron required"))?;
            let _ = normalize_cron_expr(expr)?;
            let tz = schedule
                .tz
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string());
            Ok((kind, None, None, Some(expr.to_string()), tz))
        }
        _ => Err(anyhow!("unsupported schedule kind")),
    }
}

fn normalize_session_target(input: Option<&str>) -> String {
    let raw = input.unwrap_or("main").trim().to_lowercase();
    if raw == "isolated" {
        "isolated".to_string()
    } else {
        "main".to_string()
    }
}

fn compute_next_run_at(
    kind: &str,
    schedule_at: Option<&str>,
    schedule_every_ms: Option<i64>,
    schedule_cron: Option<&str>,
    schedule_tz: Option<&str>,
    created_at: f64,
    now: f64,
) -> Option<f64> {
    match kind.trim().to_lowercase().as_str() {
        "at" => schedule_at
            .and_then(parse_rfc3339)
            .map(|dt| dt.timestamp_millis() as f64 / 1000.0),
        "every" => {
            let every_ms = schedule_every_ms.unwrap_or(0).max(1);
            if every_ms <= 0 {
                return None;
            }
            let anchor_ms = (created_at * 1000.0) as i64;
            let now_ms = (now * 1000.0) as i64;
            if now_ms < anchor_ms {
                return Some(anchor_ms as f64 / 1000.0);
            }
            let elapsed = now_ms - anchor_ms;
            let steps = (elapsed + every_ms - 1) / every_ms;
            let next_ms = anchor_ms + steps * every_ms;
            Some(next_ms as f64 / 1000.0)
        }
        "cron" => compute_next_cron(schedule_cron, schedule_tz, now),
        _ => None,
    }
}

fn compute_next_cron(expr: Option<&str>, tz: Option<&str>, now: f64) -> Option<f64> {
    let expr = expr.and_then(|value| normalize_cron_expr(value).ok())?;
    let schedule = Schedule::from_str(&expr).ok()?;
    let now_ms = (now * 1000.0) as i64;
    let base = DateTime::<Utc>::from_timestamp_millis(now_ms)?;
    if let Some(tz) = tz {
        if let Ok(tz) = Tz::from_str(tz) {
            let base_tz = base.with_timezone(&tz);
            let next = schedule.after(&base_tz).next()?;
            return Some(next.with_timezone(&Utc).timestamp_millis() as f64 / 1000.0);
        }
    }
    let next = schedule.after(&base).next()?;
    Some(next.timestamp_millis() as f64 / 1000.0)
}

fn normalize_cron_expr(expr: &str) -> Result<String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("cron expression empty"));
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() == 5 {
        Ok(format!("0 {trimmed}"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(parsed.with_timezone(&Utc));
    }
    if let Ok(timestamp) = trimmed.parse::<f64>() {
        let ts = if timestamp > 1e12 {
            timestamp as i64
        } else {
            (timestamp * 1000.0) as i64
        };
        return DateTime::<Utc>::from_timestamp_millis(ts);
    }
    None
}

fn extract_payload_message(payload: Option<&Value>) -> Option<String> {
    let Some(payload) = payload else {
        return None;
    };
    match payload {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Object(map) => map
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string()),
        _ => None,
    }
}

fn cron_job_to_value(record: &CronJobRecord) -> Value {
    json!({
        "job_id": record.job_id,
        "user_id": record.user_id,
        "session_id": record.session_id,
        "agent_id": record.agent_id,
        "name": record.name,
        "session_target": record.session_target,
        "payload": record.payload,
        "deliver": record.deliver,
        "enabled": record.enabled,
        "delete_after_run": record.delete_after_run,
        "dedupe_key": record.dedupe_key,
        "schedule": {
            "kind": record.schedule_kind,
            "at": record.schedule_at,
            "every_ms": record.schedule_every_ms,
            "cron": record.schedule_cron,
            "tz": record.schedule_tz,
        },
        "next_run_at": record.next_run_at,
        "next_run_at_text": format_ts(record.next_run_at),
        "running_at": record.running_at,
        "running_at_text": format_ts(record.running_at),
        "last_run_at": record.last_run_at,
        "last_run_at_text": format_ts(record.last_run_at),
        "last_status": record.last_status,
        "last_error": record.last_error,
        "created_at": record.created_at,
        "created_at_text": format_ts(Some(record.created_at)),
        "updated_at": record.updated_at,
        "updated_at_text": format_ts(Some(record.updated_at))
    })
}

fn cron_run_to_value(record: &CronRunRecord) -> Value {
    json!({
        "run_id": record.run_id,
        "job_id": record.job_id,
        "user_id": record.user_id,
        "session_id": record.session_id,
        "agent_id": record.agent_id,
        "trigger": record.trigger,
        "status": record.status,
        "summary": record.summary,
        "error": record.error,
        "duration_ms": record.duration_ms,
        "created_at": record.created_at,
        "created_at_text": format_ts(Some(record.created_at))
    })
}

fn format_ts(value: Option<f64>) -> Option<String> {
    let ts = value?;
    let millis = (ts * 1000.0) as i64;
    DateTime::<Utc>::from_timestamp_millis(millis).map(|dt| dt.to_rfc3339())
}

fn resolve_agent_record(
    user_store: &UserStore,
    user: &UserAccountRecord,
    agent_id: Option<&str>,
) -> Option<UserAgentRecord> {
    let agent_id = agent_id.map(str::trim).filter(|value| !value.is_empty())?;
    let record = user_store.get_user_agent_by_id(agent_id).ok().flatten()?;
    let access = user_store
        .get_user_agent_access(&user.user_id)
        .ok()
        .flatten();
    if is_agent_allowed(user, access.as_ref(), &record) {
        Some(record)
    } else {
        None
    }
}

fn resolve_session_tool_overrides(
    record: &ChatSessionRecord,
    agent: Option<&UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else {
        agent
            .map(|record| record.tool_names.clone())
            .unwrap_or_default()
    }
}

fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == TOOL_OVERRIDE_NONE {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![TOOL_OVERRIDE_NONE.to_string()]
    } else {
        output
    }
}

fn apply_tool_overrides(allowed: HashSet<String>, overrides: &[String]) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let override_set: HashSet<String> = overrides
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    allowed
        .intersection(&override_set)
        .cloned()
        .collect::<HashSet<_>>()
}

fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec![TOOL_OVERRIDE_NONE.to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

fn should_auto_title(title: &str) -> bool {
    let cleaned = title.trim();
    cleaned.is_empty() || cleaned == "新会话" || cleaned == "未命名会话"
}

fn build_session_title(content: &str) -> Option<String> {
    let cleaned = content.trim().replace('\n', " ");
    if cleaned.is_empty() {
        return None;
    }
    let mut output = cleaned;
    if output.chars().count() > 20 {
        output = output.chars().take(20).collect::<String>();
        output.push_str("...");
    }
    Some(output)
}

fn build_virtual_user(user_id: &str) -> UserAccountRecord {
    let now = now_ts();
    UserAccountRecord {
        user_id: user_id.to_string(),
        username: user_id.to_string(),
        email: None,
        password_hash: String::new(),
        roles: vec!["user".to_string()],
        status: "active".to_string(),
        access_level: "A".to_string(),
        unit_id: None,
        daily_quota: 0,
        daily_quota_used: 0,
        daily_quota_date: None,
        is_demo: false,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut output = trimmed.to_string();
    if output.chars().count() > max_chars {
        output = output.chars().take(max_chars).collect::<String>();
        output.push_str("...");
    }
    output
}

fn is_user_busy(err: &anyhow::Error) -> bool {
    err.downcast_ref::<crate::orchestrator::OrchestratorError>()
        .map(|err| err.code() == "USER_BUSY")
        .unwrap_or(false)
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[derive(Clone)]
struct CronRuntime {
    config: Config,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Arc<Orchestrator>,
    user_store: Arc<UserStore>,
    user_tool_manager: Arc<UserToolManager>,
    skills: Arc<RwLock<SkillRegistry>>,
}

impl CronRuntime {
    async fn from_scheduler(scheduler: &CronScheduler) -> Self {
        let config = scheduler.config_store.get().await;
        Self {
            config,
            storage: scheduler.storage.clone(),
            orchestrator: scheduler.orchestrator.clone(),
            user_store: scheduler.user_store.clone(),
            user_tool_manager: scheduler.user_tool_manager.clone(),
            skills: scheduler.skills.clone(),
        }
    }

    fn from_parts(
        config: Config,
        storage: Arc<dyn StorageBackend>,
        orchestrator: Arc<Orchestrator>,
        user_store: Arc<UserStore>,
        user_tool_manager: Arc<UserToolManager>,
        skills: Arc<RwLock<SkillRegistry>>,
    ) -> Self {
        Self {
            config,
            storage,
            orchestrator,
            user_store,
            user_tool_manager,
            skills,
        }
    }

    async fn execute_job(&self, job: CronJobRecord, trigger: &str) {
        let started = Instant::now();
        let start_ts = now_ts();
        let mut status = "ok".to_string();
        let mut summary = None;
        let mut error_msg = None;
        let mut run_session_id = job.session_id.clone();
        let message = extract_payload_message(Some(&job.payload));
        let message = match message {
            Some(text) => text,
            None => {
                status = "skipped".to_string();
                error_msg = Some("payload.message is empty".to_string());
                self.finish_job(
                    job, trigger, &status, &summary, &error_msg, start_ts, started,
                )
                .await;
                return;
            }
        };

        let is_isolated = job.session_target.trim().eq_ignore_ascii_case("isolated");
        if is_isolated {
            run_session_id = Uuid::new_v4().simple().to_string();
        }

        let run_result = self
            .run_request_when_idle(
                &job.user_id,
                &run_session_id,
                job.agent_id.as_deref(),
                &message,
                is_isolated.then_some(&job.session_id),
            )
            .await;

        match run_result {
            Ok(response) => {
                summary = Some(truncate_text(&response.answer, SUMMARY_MAX_CHARS));
                if is_isolated {
                    let deliver_message = response.answer.clone();
                    let deliver_result = self
                        .run_request_when_idle(
                            &job.user_id,
                            &job.session_id,
                            job.agent_id.as_deref(),
                            &deliver_message,
                            None,
                        )
                        .await;
                    if let Err(err) = deliver_result {
                        status = "error".to_string();
                        error_msg = Some(format!("deliver failed: {err}"));
                    }
                }
            }
            Err(err) => {
                status = "error".to_string();
                error_msg = Some(err.to_string());
            }
        }

        self.finish_job(
            job, trigger, &status, &summary, &error_msg, start_ts, started,
        )
        .await;
    }

    async fn run_request_when_idle(
        &self,
        user_id: &str,
        session_id: &str,
        agent_id: Option<&str>,
        message: &str,
        parent_session_id: Option<&str>,
    ) -> Result<crate::schemas::WunderResponse> {
        let request = self
            .build_request(user_id, session_id, agent_id, message, parent_session_id)
            .await?;
        let retry_ms = self.config.cron.idle_retry_ms.max(200);
        loop {
            match self.run_stream_request(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(err) => {
                    if is_user_busy(&err) {
                        sleep(Duration::from_millis(retry_ms)).await;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    async fn run_stream_request(
        &self,
        request: WunderRequest,
    ) -> Result<crate::schemas::WunderResponse> {
        let session_id = request
            .session_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let stream = self.orchestrator.stream(request).await?;
        tokio::pin!(stream);
        let mut answer: Option<String> = None;
        let mut usage: Option<crate::schemas::TokenUsage> = None;
        let mut stop_reason: Option<String> = None;
        let mut error_msg: Option<String> = None;
        while let Some(item) = stream.next().await {
            let event = match item {
                Ok(value) => value,
                Err(_) => continue,
            };
            match event.event.as_str() {
                "final" => {
                    if let Some(payload) = event.data.get("data") {
                        answer = payload
                            .get("answer")
                            .and_then(Value::as_str)
                            .map(|text| text.to_string());
                        usage = payload
                            .get("usage")
                            .cloned()
                            .and_then(|value| serde_json::from_value(value).ok());
                        stop_reason = payload
                            .get("stop_reason")
                            .and_then(Value::as_str)
                            .map(|text| text.to_string());
                    }
                }
                "error" => {
                    if let Some(payload) = event.data.get("data") {
                        if let Some(message) = payload
                            .get("message")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                        {
                            error_msg = Some(message.to_string());
                        } else if let Some(message) = payload
                            .get("error")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                        {
                            error_msg = Some(message.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(message) = error_msg {
            return Err(anyhow!(message));
        }
        let Some(answer) = answer else {
            return Err(anyhow!("stream finished without final response"));
        };
        Ok(crate::schemas::WunderResponse {
            session_id,
            answer,
            usage,
            stop_reason,
            uid: None,
            a2ui: None,
        })
    }

    async fn build_request(
        &self,
        user_id: &str,
        session_id: &str,
        agent_id: Option<&str>,
        content: &str,
        parent_session_id: Option<&str>,
    ) -> Result<WunderRequest> {
        let cleaned_session = session_id.trim();
        if cleaned_session.is_empty() {
            return Err(anyhow!(i18n::t("error.session_not_found")));
        }
        let message = content.trim();
        if message.is_empty() {
            return Err(anyhow!(i18n::t("error.content_required")));
        }
        let now = now_ts();
        let user = self
            .user_store
            .get_user_by_id(user_id)?
            .unwrap_or_else(|| build_virtual_user(user_id));
        let mut record = self
            .user_store
            .get_chat_session(&user.user_id, cleaned_session)?
            .unwrap_or_else(|| ChatSessionRecord {
                session_id: cleaned_session.to_string(),
                user_id: user.user_id.clone(),
                title: DEFAULT_SESSION_TITLE.to_string(),
                created_at: now,
                updated_at: now,
                last_message_at: now,
                agent_id: agent_id.map(|value| value.to_string()),
                tool_overrides: Vec::new(),
                parent_session_id: parent_session_id.map(|value| value.to_string()),
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            });
        if record.agent_id.is_none() {
            record.agent_id = agent_id.map(|value| value.to_string());
        }
        if record.parent_session_id.is_none() && parent_session_id.is_some() {
            record.parent_session_id = parent_session_id.map(|value| value.to_string());
        }
        self.user_store.upsert_chat_session(&record)?;

        let agent_record =
            resolve_agent_record(&self.user_store, &user, record.agent_id.as_deref());
        let user_context = self.build_user_tool_context(&user.user_id).await;
        let mut allowed = compute_allowed_tool_names(&user, &user_context);
        let overrides = resolve_session_tool_overrides(&record, agent_record.as_ref());
        allowed = apply_tool_overrides(allowed, &overrides);
        let tool_names = finalize_tool_names(allowed);
        let agent_prompt = agent_record
            .as_ref()
            .map(|record| record.system_prompt.trim().to_string())
            .filter(|value| !value.is_empty());

        if should_auto_title(&record.title) {
            if let Some(title) = build_session_title(message) {
                let _ = self.user_store.update_chat_session_title(
                    &user.user_id,
                    cleaned_session,
                    &title,
                    now,
                );
            }
        }
        let _ = self
            .user_store
            .touch_chat_session(&user.user_id, cleaned_session, now, now);

        Ok(WunderRequest {
            user_id: user.user_id.clone(),
            question: message.to_string(),
            tool_names,
            skip_tool_calls: false,
            stream: true,
            debug_payload: false,
            session_id: Some(cleaned_session.to_string()),
            agent_id: record.agent_id.clone(),
            model_name: None,
            language: None,
            config_overrides: None,
            agent_prompt,
            attachments: None,
            is_admin: UserStore::is_admin(&user),
        })
    }

    async fn build_user_tool_context(&self, user_id: &str) -> UserToolContext {
        let skills = self.skills.read().await.clone();
        let bindings = self
            .user_tool_manager
            .build_bindings(&self.config, &skills, user_id);
        let tool_access = self
            .user_store
            .get_user_tool_access(user_id)
            .unwrap_or(None);
        UserToolContext {
            config: self.config.clone(),
            skills,
            bindings,
            tool_access,
        }
    }

    async fn finish_job(
        &self,
        job: CronJobRecord,
        trigger: &str,
        status: &str,
        summary: &Option<String>,
        error_msg: &Option<String>,
        start_ts: f64,
        started: Instant,
    ) {
        let duration_ms = started.elapsed().as_millis() as i64;
        let now = now_ts();
        let storage = self.storage.clone();
        let job_clone = job.clone();
        let trigger = trigger.to_string();
        let status = status.to_string();
        let summary = summary.clone();
        let error_msg = error_msg.clone();
        match tokio::task::spawn_blocking(move || {
            persist_cron_run_and_update_job(
                storage.as_ref(),
                job_clone,
                trigger,
                status,
                summary,
                error_msg,
                start_ts,
                duration_ms,
                now,
            )
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                error!("failed to write cron run: {}", err);
            }
            Err(err) => {
                error!("failed to write cron run: {}", err);
            }
        }
    }
}

#[doc(hidden)]
pub fn persist_cron_run_and_update_job(
    storage: &dyn StorageBackend,
    job: CronJobRecord,
    trigger: String,
    status: String,
    summary: Option<String>,
    error_msg: Option<String>,
    start_ts: f64,
    duration_ms: i64,
    now: f64,
) -> Result<()> {
    let run_record = CronRunRecord {
        run_id: Uuid::new_v4().simple().to_string(),
        job_id: job.job_id.clone(),
        user_id: job.user_id.clone(),
        session_id: Some(job.session_id.clone()),
        agent_id: job.agent_id.clone(),
        trigger,
        status: status.clone(),
        summary,
        error: error_msg.clone(),
        duration_ms,
        created_at: now,
    };
    storage.insert_cron_run(&run_record)?;
    if job.delete_after_run && status == "ok" {
        let _ = storage.delete_cron_job(&job.user_id, &job.job_id);
        return Ok(());
    }
    let Some(mut record) = storage.get_cron_job(&job.user_id, &job.job_id)? else {
        return Ok(());
    };
    record.running_at = None;
    record.last_run_at = Some(start_ts);
    record.last_status = Some(status);
    record.last_error = error_msg;
    record.updated_at = now;
    let mut next_run_at = None;
    if record.enabled {
        next_run_at = compute_next_run_at(
            &record.schedule_kind,
            record.schedule_at.as_deref(),
            record.schedule_every_ms,
            record.schedule_cron.as_deref(),
            record.schedule_tz.as_deref(),
            record.created_at,
            now,
        );
    }
    if record.schedule_kind.eq_ignore_ascii_case("at") {
        next_run_at = None;
    }
    record.next_run_at = next_run_at;
    if record.schedule_kind.eq_ignore_ascii_case("at")
        && record.last_status.as_deref() == Some("ok")
    {
        record.enabled = false;
    }
    storage.upsert_cron_job(&record)?;
    Ok(())
}

#[derive(Debug, Deserialize, Clone)]
pub struct CronJobInput {
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub schedule: Option<CronScheduleInput>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default)]
    pub deliver: Option<Value>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub delete_after_run: Option<bool>,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CronScheduleInput {
    pub kind: String,
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub every_ms: Option<i64>,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub tz: Option<String>,
}

#[derive(Clone)]
pub struct CronScheduler {
    config_store: ConfigStore,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Arc<Orchestrator>,
    user_store: Arc<UserStore>,
    user_tool_manager: Arc<UserToolManager>,
    skills: Arc<RwLock<SkillRegistry>>,
}

impl CronScheduler {
    pub fn new(
        config_store: ConfigStore,
        storage: Arc<dyn StorageBackend>,
        orchestrator: Arc<Orchestrator>,
        user_store: Arc<UserStore>,
        user_tool_manager: Arc<UserToolManager>,
        skills: Arc<RwLock<SkillRegistry>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config_store,
            storage,
            orchestrator,
            user_store,
            user_tool_manager,
            skills,
        })
    }

    pub fn start(self: &Arc<Self>) {
        let scheduler = Arc::clone(self);
        tokio::spawn(async move {
            scheduler.run_loop().await;
        });
    }

    async fn run_loop(self: Arc<Self>) {
        let _ = self.reset_running_jobs().await;
        loop {
            let config = self.config_store.get().await;
            let cron_cfg = config.cron.clone();
            if !cron_cfg.enabled {
                sleep(Duration::from_millis(cron_cfg.poll_interval_ms.max(500))).await;
                continue;
            }
            let now = now_ts();
            let running = self.count_running_jobs().await.unwrap_or(0);
            let max_runs = cron_cfg.max_concurrent_runs.max(1) as i64;
            let capacity = (max_runs - running).max(0);
            if capacity > 0 {
                let jobs = self.claim_due_jobs(now, capacity).await.unwrap_or_default();
                for job in jobs {
                    let scheduler = Arc::clone(&self);
                    tokio::spawn(async move {
                        scheduler.execute_job(job, "timer").await;
                    });
                }
            }
            let next = self.get_next_cron_run_at(now).await.unwrap_or(None);
            let mut sleep_ms = cron_cfg.poll_interval_ms.max(200);
            if let Some(next_at) = next {
                let delta = ((next_at - now) * 1000.0).ceil() as i64;
                if delta > 0 {
                    sleep_ms = sleep_ms.min(delta as u64);
                }
            }
            sleep(Duration::from_millis(sleep_ms)).await;
        }
    }

    async fn reset_running_jobs(&self) -> Result<()> {
        let storage = self.storage.clone();
        tokio::task::spawn_blocking(move || storage.reset_cron_jobs_running())
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
        Ok(())
    }

    async fn count_running_jobs(&self) -> Result<i64> {
        let storage = self.storage.clone();
        let count = tokio::task::spawn_blocking(move || storage.count_running_cron_jobs())
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
        Ok(count)
    }

    async fn claim_due_jobs(&self, now: f64, limit: i64) -> Result<Vec<CronJobRecord>> {
        let storage = self.storage.clone();
        let jobs = tokio::task::spawn_blocking(move || storage.claim_due_cron_jobs(now, limit))
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
        Ok(jobs)
    }

    async fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>> {
        let storage = self.storage.clone();
        let next = tokio::task::spawn_blocking(move || storage.get_next_cron_run_at(now))
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
        Ok(next)
    }

    async fn execute_job(&self, job: CronJobRecord, trigger: &str) {
        let runtime = CronRuntime::from_scheduler(self).await;
        runtime.execute_job(job, trigger).await;
    }
}
