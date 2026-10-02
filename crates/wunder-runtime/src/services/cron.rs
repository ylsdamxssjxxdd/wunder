mod policy;

use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::core::long_task;
use crate::core::{blocking, runtime_metrics};
use crate::i18n;
use crate::orchestrator::Orchestrator;
use crate::schemas::WunderRequest;
use crate::services::agent_execution::{
    apply_tool_overrides, finalize_tool_names, resolve_agent_tool_defaults, resolve_chat_model_name,
};
use crate::services::cron_schedule::{
    normalize_every_ms, parse_schedule_text, validate_cron_expr, validate_message, validate_name,
    validate_schedule_at, ParsedScheduleText, MIN_EVERY_MS,
};
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
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_stream::StreamExt;
use tracing::error;
use uuid::Uuid;

use self::policy::{compute_error_backoff_ms, compute_scheduler_sleep_ms};

async fn run_cron_db<T, F>(label: &'static str, task: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    blocking::run_db(label, task).await
}

const DEFAULT_SESSION_TITLE: &str = "新会话";
const SUMMARY_MAX_CHARS: usize = 200;
const DEFAULT_MAX_CONSECUTIVE_FAILURES: usize = 5;
const AUTO_DISABLED_REASON_MAX_CHARS: usize = 240;
const MIN_CRON_LEASE_TTL_MS: u64 = 5_000;
const MIN_CRON_LEASE_HEARTBEAT_MS: u64 = 1_000;

type NormalizedSchedule = (
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone)]
struct CronSessionRouting {
    run_session_id: String,
    deliver_session_id: String,
    parent_session_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct CronWakeSignal {
    notify: Arc<Notify>,
}

impl CronWakeSignal {
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn notify(&self) {
        self.notify.notify_one();
    }

    async fn wait(&self) {
        self.notify.notified().await;
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CronActionRequest {
    pub action: String,
    #[serde(default)]
    pub job: Option<CronJobInput>,
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_cron_action(
    config: Config,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Option<Arc<Orchestrator>>,
    wake_signal: Option<CronWakeSignal>,
    user_store: Arc<UserStore>,
    user_tool_manager: Arc<UserToolManager>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_id: &str,
    session_id: Option<&str>,
    agent_id: Option<&str>,
    payload: CronActionRequest,
) -> Result<Value> {
    let action = payload.action.trim().to_lowercase();
    // Do not acknowledge an executable schedule when the worker is disabled.
    // Read/delete/disable remain available for inspecting and cleaning up jobs.
    if !config.cron.enabled && matches!(action.as_str(), "add" | "update" | "enable" | "run") {
        return Err(anyhow!("Scheduled task execution is disabled (cron.enabled=false). Enable the scheduler before scheduling or running tasks."));
    }
    let scoped_agent_id = resolve_scoped_agent_id(agent_id, payload.job.as_ref());
    let now = now_ts();
    match action.as_str() {
        "status" => {
            let global_running = {
                let storage = storage.clone();
                run_cron_db("cron.status.count_running", move || {
                    storage.count_running_cron_jobs(now)
                })
                .await?
            };
            let global_next_run_at = {
                let storage = storage.clone();
                run_cron_db("cron.status.next_run_at", move || {
                    storage.get_next_cron_run_at(now)
                })
                .await?
            };
            let user_jobs = {
                let storage = storage.clone();
                let cleaned_user = user_id.trim().to_string();
                run_cron_db("cron.status.list_user_jobs", move || {
                    storage.list_cron_jobs(&cleaned_user, true)
                })
                .await?
            };
            let scoped_jobs = filter_jobs_by_agent_scope(user_jobs, scoped_agent_id.as_deref());
            let user_total_jobs = scoped_jobs.len();
            let user_enabled_jobs = scoped_jobs.iter().filter(|job| job.enabled).count();
            let user_running_jobs = scoped_jobs
                .iter()
                .filter(|job| cron_job_is_running(job, now))
                .count();
            let user_next_run_at = scoped_jobs
                .iter()
                .filter(|job| job.enabled)
                .filter_map(|job| job.next_run_at)
                .min_by(f64::total_cmp);
            Ok(json!({
                "action": "status",
                "scheduler": {
                    "enabled": config.cron.enabled,
                    "poll_interval_ms": config.cron.poll_interval_ms,
                    "max_idle_sleep_ms": config.cron.max_idle_sleep_ms,
                    "max_concurrent_runs": config.cron.max_concurrent_runs,
                    "idle_retry_ms": config.cron.idle_retry_ms,
                    "max_busy_wait_ms": config.cron.max_busy_wait_ms,
                    "max_consecutive_failures": config.cron.max_consecutive_failures,
                    "lease_ttl_ms": effective_cron_lease_ttl_ms(&config),
                    "lease_heartbeat_ms": effective_cron_lease_heartbeat_ms(&config),
                    "running_jobs": global_running,
                    "next_run_at": global_next_run_at,
                    "next_run_at_text": format_ts(global_next_run_at),
                    "now": now,
                    "now_text": format_ts(Some(now)),
                },
                "user_jobs": {
                    "total": user_total_jobs,
                    "enabled": user_enabled_jobs,
                    "running": user_running_jobs,
                    "next_run_at": user_next_run_at,
                    "next_run_at_text": format_ts(user_next_run_at),
                }
            }))
        }
        "list" => {
            let storage = storage.clone();
            let cleaned = user_id.trim().to_string();
            let jobs = run_cron_db("cron.action.list_jobs", move || {
                storage.list_cron_jobs(&cleaned, true)
            })
            .await?;
            let jobs = filter_jobs_by_agent_scope(jobs, scoped_agent_id.as_deref());
            let items = jobs.iter().map(cron_job_to_value).collect::<Vec<_>>();
            Ok(
                json!({ "action": "list", "jobs": items, "scheduler": { "enabled": config.cron.enabled } }),
            )
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
            let job = run_cron_db("cron.action.get_job", move || {
                storage.get_cron_job(&cleaned_user, &cleaned_job)
            })
            .await?;
            let Some(job) = job else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            if !cron_job_matches_agent_scope(&job, scoped_agent_id.as_deref()) {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            }
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
                    run_cron_db("cron.action.get_job_by_dedupe", move || {
                        storage.get_cron_job_by_dedupe_key(&cleaned_user, &key)
                    })
                    .await?
                };
                if let Some(mut record) = existing.filter(|record| {
                    cron_job_matches_agent_scope(record, scoped_agent_id.as_deref())
                }) {
                    apply_job_patch(
                        &mut record,
                        &input,
                        job_session_id.as_str(),
                        scoped_agent_id.as_deref(),
                        now,
                        true,
                    )?;
                    let storage = storage.clone();
                    let record_clone = record.clone();
                    run_cron_db("cron.action.upsert_job", move || {
                        storage.upsert_cron_job(&record_clone)
                    })
                    .await?;
                    if let Some(signal) = wake_signal.clone() {
                        signal.notify();
                    }
                    return Ok(json!({
                        "action": "update",
                        "job": cron_job_to_value(&record),
                        "deduped": true
                    }));
                }
            }
            let record = build_job_record(
                user_id,
                job_session_id.as_str(),
                scoped_agent_id.as_deref(),
                now,
                input,
            )?;
            let storage = storage.clone();
            let record_clone = record.clone();
            run_cron_db("cron.action.upsert_job", move || {
                storage.upsert_cron_job(&record_clone)
            })
            .await?;
            if let Some(signal) = wake_signal.clone() {
                signal.notify();
            }
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
                run_cron_db("cron.action.update.get_job", move || {
                    storage.get_cron_job(&cleaned_user, &cleaned_job)
                })
                .await?
            };
            let Some(mut record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            if !cron_job_matches_agent_scope(&record, scoped_agent_id.as_deref()) {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            }
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
                scoped_agent_id.as_deref(),
                now,
                false,
            )?;
            let storage = storage.clone();
            let record_clone = record.clone();
            run_cron_db("cron.action.upsert_job", move || {
                storage.upsert_cron_job(&record_clone)
            })
            .await?;
            if let Some(signal) = wake_signal.clone() {
                signal.notify();
            }
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
                run_cron_db("cron.action.toggle.get_job", move || {
                    storage.get_cron_job(&cleaned_user, &cleaned_job)
                })
                .await?
            };
            let Some(mut record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            if !cron_job_matches_agent_scope(&record, scoped_agent_id.as_deref()) {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            }
            record.enabled = action == "enable";
            if record.enabled {
                record.consecutive_failures = 0;
                record.auto_disabled_reason = None;
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
                clear_cron_job_lease(&mut record);
            }
            record.updated_at = now;
            let storage = storage.clone();
            let record_clone = record.clone();
            run_cron_db("cron.action.upsert_job", move || {
                storage.upsert_cron_job(&record_clone)
            })
            .await?;
            if let Some(signal) = wake_signal.clone() {
                signal.notify();
            }
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
            let cleaned_user = user_id.trim().to_string();
            let cleaned_job = job_id.to_string();
            let existing = {
                let storage = storage.clone();
                let lookup_user = cleaned_user.clone();
                let lookup_job = cleaned_job.clone();
                run_cron_db("cron.action.lookup_job", move || {
                    storage.get_cron_job(&lookup_user, &lookup_job)
                })
                .await?
            };
            let Some(record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            if !cron_job_matches_agent_scope(&record, scoped_agent_id.as_deref()) {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            }
            let storage = storage.clone();
            let removed = run_cron_db("cron.action.delete_job", move || {
                storage.delete_cron_job(&cleaned_user, &cleaned_job)
            })
            .await?;
            if removed > 0 {
                if let Some(signal) = wake_signal.clone() {
                    signal.notify();
                }
            }
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
                run_cron_db("cron.action.run.get_job", move || {
                    storage.get_cron_job(&cleaned_user, &cleaned_job)
                })
                .await?
            };
            let Some(mut record) = existing else {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            };
            if !cron_job_matches_agent_scope(&record, scoped_agent_id.as_deref()) {
                return Err(anyhow!(i18n::t("error.task_not_found")));
            }

            if cron_job_is_running(&record, now) {
                return Ok(json!({
                    "action": "run",
                    "queued": false,
                    "reason": "running",
                    "job": cron_job_to_value(&record)
                }));
            }
            let orchestrator = orchestrator
                .ok_or_else(|| anyhow!("task runtime is unavailable; job was not started"))?;
            let manual_runner_id = format!("manual_{}", Uuid::new_v4().simple());
            let manual_run_token = Uuid::new_v4().simple().to_string();
            assign_cron_job_lease(
                &mut record,
                manual_runner_id,
                manual_run_token,
                now,
                cron_lease_expires_at(&config, now),
            );
            record.updated_at = now;
            let record_for_upsert = record.clone();
            let record_for_response = record.clone();
            let storage_for_upsert = storage.clone();
            run_cron_db("cron.action.run.upsert_job", move || {
                storage_for_upsert.upsert_cron_job(&record_for_upsert)
            })
            .await?;
            if let Some(signal) = wake_signal.clone() {
                signal.notify();
            }
            let runtime = CronRuntime::from_parts(
                config,
                storage.clone(),
                orchestrator,
                wake_signal.clone().unwrap_or_default(),
                user_store,
                user_tool_manager,
                skills,
            );
            let handle = tokio::runtime::Handle::current();
            let _handle = long_task::spawn("cron.manual.execute", async move {
                let _ = blocking::run_external("cron.action.run.execute_manual", move || {
                    handle.block_on(runtime.execute_job(record, "manual"));
                    Ok(())
                })
                .await;
            });
            Ok(json!({
                "action": "run",
                "queued": true,
                "resolved_session_id": record_for_response.session_id,
                "delivery_session_id": record_for_response.session_id,
                "job": cron_job_to_value(&record_for_response)
            }))
        }
        _ => Err(anyhow!("unsupported action: {}", payload.action)),
    }
}

pub async fn list_cron_runs(
    storage: Arc<dyn StorageBackend>,
    user_id: &str,
    job_id: &str,
    scoped_agent_id: Option<&str>,
    limit: i64,
) -> Result<Value> {
    let cleaned_user = user_id.trim().to_string();
    let cleaned_job = job_id.trim().to_string();
    let safe_limit = limit.clamp(1, 200);
    let existing = {
        let storage = storage.clone();
        let cleaned_user = cleaned_user.clone();
        let cleaned_job = cleaned_job.clone();
        run_cron_db("cron.runs.get_job", move || {
            storage.get_cron_job(&cleaned_user, &cleaned_job)
        })
        .await?
    };
    let Some(record) = existing else {
        return Err(anyhow!(i18n::t("error.task_not_found")));
    };
    if !cron_job_matches_agent_scope(&record, scoped_agent_id) {
        return Err(anyhow!(i18n::t("error.task_not_found")));
    }
    let storage = storage.clone();
    let runs = {
        let job_id = cleaned_job.clone();
        run_cron_db("cron.runs.list", move || {
            storage.list_cron_runs(&cleaned_user, &job_id, safe_limit)
        })
        .await?
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
    let (schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz) =
        normalize_schedule_input_with_text(
            input.schedule.as_ref(),
            input.schedule_text.as_deref(),
        )?;
    let payload = input.payload.unwrap_or(Value::Null);
    let message = extract_payload_message(Some(&payload))
        .ok_or_else(|| anyhow!("payload.message required"))?;
    validate_message(&message)?;
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
    if let Some(name) = name.as_deref() {
        validate_name(name)?;
    }
    validate_schedule_fields(
        &schedule_kind,
        schedule_at.as_deref(),
        schedule_every_ms,
        schedule_cron.as_deref(),
        now,
    )?;
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
        runner_id: None,
        run_token: None,
        heartbeat_at: None,
        lease_expires_at: None,
        last_run_at: None,
        last_status: None,
        last_error: None,
        consecutive_failures: 0,
        auto_disabled_reason: None,
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
    let mut schedule_changed = false;
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
        record.payload = payload.clone();
    }
    if let Some(message) = extract_payload_message(Some(&record.payload)) {
        validate_message(&message)?;
    } else if !allow_missing_payload {
        return Err(anyhow!("payload.message required"));
    }
    if let Some(deliver) = input.deliver.as_ref() {
        record.deliver = Some(deliver.clone());
    }
    if let Some(enabled) = input.enabled {
        record.enabled = enabled;
        if enabled {
            record.consecutive_failures = 0;
            record.auto_disabled_reason = None;
        }
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
        schedule_changed = true;
    } else if let Some(schedule_text) = input.schedule_text.as_deref() {
        let (kind, at, every, cron, tz) =
            normalize_schedule_input_with_text(None, Some(schedule_text))?;
        record.schedule_kind = kind;
        record.schedule_at = at;
        record.schedule_every_ms = every;
        record.schedule_cron = cron;
        record.schedule_tz = tz;
        schedule_changed = true;
    }
    if let Some(name) = record.name.as_deref() {
        validate_name(name)?;
    }
    if schedule_changed {
        validate_schedule_fields(
            &record.schedule_kind,
            record.schedule_at.as_deref(),
            record.schedule_every_ms,
            record.schedule_cron.as_deref(),
            now,
        )?;
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
        clear_cron_job_lease(record);
    }
    Ok(())
}

fn normalize_schedule_input(schedule: &CronScheduleInput) -> Result<NormalizedSchedule> {
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
            let raw_every_ms = schedule.every_ms.unwrap_or(MIN_EVERY_MS);
            let every_ms = normalize_every_ms(raw_every_ms)?;
            let start_at = schedule
                .at
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(start_at) = start_at {
                if parse_rfc3339(start_at).is_none() {
                    return Err(anyhow!("invalid schedule.at"));
                }
            }
            Ok((
                kind,
                start_at.map(|value| value.to_string()),
                Some(every_ms),
                None,
                None,
            ))
        }
        "cron" => {
            let expr = schedule
                .cron
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("schedule.cron required"))?;
            validate_cron_expr(expr)?;
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

fn normalize_schedule_input_with_text(
    schedule: Option<&CronScheduleInput>,
    schedule_text: Option<&str>,
) -> Result<NormalizedSchedule> {
    if let Some(schedule) = schedule {
        return normalize_schedule_input(schedule);
    }
    let text = schedule_text
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("schedule required"))?;
    let parsed = parse_schedule_text(text)?;
    match parsed {
        ParsedScheduleText::EveryMs(ms) => {
            let every_ms = normalize_every_ms(ms)?;
            Ok(("every".to_string(), None, Some(every_ms), None, None))
        }
        ParsedScheduleText::Cron(expr) => {
            validate_cron_expr(&expr)?;
            Ok(("cron".to_string(), None, None, Some(expr), None))
        }
    }
}

fn validate_schedule_fields(
    kind: &str,
    schedule_at: Option<&str>,
    schedule_every_ms: Option<i64>,
    schedule_cron: Option<&str>,
    now: f64,
) -> Result<()> {
    let kind = kind.trim().to_lowercase();
    match kind.as_str() {
        "at" => {
            let at = schedule_at
                .and_then(parse_rfc3339)
                .ok_or_else(|| anyhow!("invalid schedule.at"))?;
            let now_dt = DateTime::<Utc>::from_timestamp_millis((now * 1000.0) as i64)
                .ok_or_else(|| anyhow!("invalid current timestamp"))?;
            if at <= now_dt {
                return Err(anyhow!("schedule.at must be in the future"));
            }
            validate_schedule_at(at, now_dt)?;
        }
        "every" => {
            let raw_every_ms = schedule_every_ms.unwrap_or(MIN_EVERY_MS);
            let _ = normalize_every_ms(raw_every_ms)?;
            if let Some(at) = schedule_at {
                if parse_rfc3339(at).is_none() {
                    return Err(anyhow!("invalid schedule.at"));
                }
            }
        }
        "cron" => {
            let expr = schedule_cron
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("schedule.cron required"))?;
            validate_cron_expr(expr)?;
            let normalized = normalize_cron_expr(expr)?;
            if Schedule::from_str(&normalized).is_err() {
                return Err(anyhow!("invalid schedule.cron"));
            }
        }
        _ => return Err(anyhow!("unsupported schedule kind")),
    }
    Ok(())
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
            let every_ms = schedule_every_ms.unwrap_or(MIN_EVERY_MS).max(MIN_EVERY_MS);
            if every_ms <= 0 {
                return None;
            }
            let anchor_ts = schedule_at
                .and_then(parse_rfc3339)
                .map(|value| value.timestamp_millis() as f64 / 1000.0)
                .unwrap_or(created_at);
            let anchor_ms = (anchor_ts * 1000.0) as i64;
            let now_ms = (now * 1000.0) as i64;
            if now_ms < anchor_ms {
                return Some(anchor_ms as f64 / 1000.0);
            }
            let elapsed = now_ms - anchor_ms;
            let steps = (elapsed / every_ms) + 1;
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
    let base = DateTime::<Utc>::from_timestamp_millis(now_ms)? + chrono::Duration::seconds(1);
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
    let payload = payload?;
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

fn effective_cron_lease_ttl_ms(config: &Config) -> u64 {
    let configured_ttl = config.cron.lease_ttl_ms.max(MIN_CRON_LEASE_TTL_MS);
    let configured_heartbeat = config
        .cron
        .lease_heartbeat_ms
        .max(MIN_CRON_LEASE_HEARTBEAT_MS);
    configured_ttl.max(configured_heartbeat.saturating_mul(2))
}

fn effective_cron_lease_heartbeat_ms(config: &Config) -> u64 {
    let ttl_ms = effective_cron_lease_ttl_ms(config);
    config
        .cron
        .lease_heartbeat_ms
        .max(MIN_CRON_LEASE_HEARTBEAT_MS)
        .min((ttl_ms / 2).max(MIN_CRON_LEASE_HEARTBEAT_MS))
}

fn cron_lease_expires_at(config: &Config, now: f64) -> f64 {
    now + effective_cron_lease_ttl_ms(config) as f64 / 1000.0
}

fn cron_job_is_running(record: &CronJobRecord, now: f64) -> bool {
    record.running_at.is_some()
        && record
            .lease_expires_at
            .map(|lease_expires_at| lease_expires_at > now)
            .unwrap_or(false)
}

fn clear_cron_job_lease(record: &mut CronJobRecord) {
    record.running_at = None;
    record.runner_id = None;
    record.run_token = None;
    record.heartbeat_at = None;
    record.lease_expires_at = None;
}

fn assign_cron_job_lease(
    record: &mut CronJobRecord,
    runner_id: String,
    run_token: String,
    now: f64,
    lease_expires_at: f64,
) {
    record.running_at = Some(now);
    record.runner_id = Some(runner_id);
    record.run_token = Some(run_token);
    record.heartbeat_at = Some(now);
    record.lease_expires_at = Some(lease_expires_at);
}

fn cron_job_matches_lease(
    record: &CronJobRecord,
    runner_id: Option<&str>,
    run_token: Option<&str>,
) -> bool {
    match (runner_id, run_token) {
        (Some(runner_id), Some(run_token)) => {
            record.runner_id.as_deref() == Some(runner_id)
                && record.run_token.as_deref() == Some(run_token)
        }
        (None, None) => true,
        _ => false,
    }
}

fn cron_job_to_value(record: &CronJobRecord) -> Value {
    let now = now_ts();
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
        "running": cron_job_is_running(record, now),
        "running_at": record.running_at,
        "running_at_text": format_ts(record.running_at),
        "heartbeat_at": record.heartbeat_at,
        "heartbeat_at_text": format_ts(record.heartbeat_at),
        "lease_expires_at": record.lease_expires_at,
        "lease_expires_at_text": format_ts(record.lease_expires_at),
        "last_run_at": record.last_run_at,
        "last_run_at_text": format_ts(record.last_run_at),
        "last_status": record.last_status,
        "last_error": record.last_error,
        "consecutive_failures": record.consecutive_failures,
        "auto_disabled_reason": record.auto_disabled_reason,
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

fn normalize_agent_id(agent_id: Option<&str>) -> Option<String> {
    agent_id
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && !value.eq_ignore_ascii_case("__default__")
                && !value.eq_ignore_ascii_case("default")
        })
        .map(|value| value.to_string())
}

fn resolve_cron_session_routing(
    storage: &dyn StorageBackend,
    job: &CronJobRecord,
) -> Result<CronSessionRouting> {
    let deliver_session_id = resolve_cron_delivery_session_id(storage, job)?;
    let is_isolated = job.session_target.trim().eq_ignore_ascii_case("isolated");
    let run_session_id = if is_isolated {
        Uuid::new_v4().simple().to_string()
    } else {
        deliver_session_id.clone()
    };
    let parent_session_id = is_isolated.then(|| deliver_session_id.clone());
    Ok(CronSessionRouting {
        run_session_id,
        deliver_session_id,
        parent_session_id,
    })
}

fn resolve_cron_delivery_session_id(
    storage: &dyn StorageBackend,
    job: &CronJobRecord,
) -> Result<String> {
    // Jobs bind to a task thread, never to the latest viewed/active agent thread.
    let record = storage
        .get_chat_session(&job.user_id, job.session_id.trim())?
        .filter(|record| !record.status.eq_ignore_ascii_case("archived"))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    if normalize_agent_id(record.agent_id.as_deref()) != normalize_agent_id(job.agent_id.as_deref())
    {
        return Err(anyhow!(
            "scheduled task agent does not match its bound thread"
        ));
    }
    Ok(record.session_id)
}

fn resolve_scoped_agent_id(
    request_agent_id: Option<&str>,
    job: Option<&CronJobInput>,
) -> Option<String> {
    normalize_agent_id(request_agent_id)
        .or_else(|| job.and_then(|job| normalize_agent_id(job.agent_id.as_deref())))
}

fn cron_job_matches_agent_scope(record: &CronJobRecord, scoped_agent_id: Option<&str>) -> bool {
    match scoped_agent_id {
        Some(agent_id) => record.agent_id.as_deref() == Some(agent_id),
        None => true,
    }
}

fn filter_jobs_by_agent_scope(
    jobs: Vec<CronJobRecord>,
    scoped_agent_id: Option<&str>,
) -> Vec<CronJobRecord> {
    match scoped_agent_id {
        Some(agent_id) => jobs
            .into_iter()
            .filter(|job| job.agent_id.as_deref() == Some(agent_id))
            .collect(),
        None => jobs,
    }
}

fn resolve_agent_record(
    user_store: &UserStore,
    storage: &dyn StorageBackend,
    user: &UserAccountRecord,
    agent_id: Option<&str>,
) -> Result<UserAgentRecord> {
    let agent_id = normalize_agent_id(agent_id);
    let Some(agent_id) = agent_id else {
        return crate::user_store::build_default_agent_record_from_storage(storage, &user.user_id);
    };
    let record = user_store
        .get_user_agent_by_id(&agent_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
    let access = user_store.get_user_agent_access(&user.user_id)?;
    if is_agent_allowed(user, access.as_ref(), &record) {
        Ok(record)
    } else {
        Err(anyhow!(i18n::t("error.agent_not_found")))
    }
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
        quota_balance: 0,
        quota_granted_total: 0,
        quota_used_total: 0,
        last_quota_grant_date: None,
        experience_total: 0,
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

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn read_cron_turn_answer(
    storage: &dyn StorageBackend,
    user: &str,
    session: &str,
    turn: &str,
) -> Result<String> {
    let mut after = -1;
    let mut answer = String::new();
    loop {
        let page = storage
            .get_thread_turn(user, session, turn, after, 100, false)?
            .ok_or_else(|| anyhow!("scheduled turn disappeared"))?;
        for item in page["items"].as_array().into_iter().flatten() {
            if item["turn_id"] == turn && item["kind"] == "assistant_message" {
                if let Some(content) = item.pointer("/payload/content").and_then(Value::as_str) {
                    if !content.is_empty() {
                        answer = content.to_string();
                    }
                }
            }
        }
        if page["has_more"] != true {
            return Ok(answer);
        }
        let next = page["next_after"]
            .as_i64()
            .filter(|next| *next > after)
            .ok_or_else(|| anyhow!("scheduled result cursor did not advance"))?;
        after = next;
    }
}

#[derive(Clone)]
struct CronRuntime {
    config: Config,
    storage: Arc<dyn StorageBackend>,
    orchestrator: Arc<Orchestrator>,
    wake_signal: CronWakeSignal,
    user_store: Arc<UserStore>,
    user_tool_manager: Arc<UserToolManager>,
    skills: Arc<RwLock<SkillRegistry>>,
}

struct CronLeaseHeartbeat {
    stop_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl CronLeaseHeartbeat {
    async fn stop(mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        let _ = self.task.await;
    }
}

impl CronRuntime {
    async fn from_scheduler(scheduler: &CronScheduler) -> Self {
        let config = scheduler.config_store.get().await;
        Self {
            config,
            storage: scheduler.storage.clone(),
            orchestrator: scheduler.orchestrator.clone(),
            wake_signal: scheduler.wake_signal.clone(),
            user_store: scheduler.user_store.clone(),
            user_tool_manager: scheduler.user_tool_manager.clone(),
            skills: scheduler.skills.clone(),
        }
    }

    fn from_parts(
        config: Config,
        storage: Arc<dyn StorageBackend>,
        orchestrator: Arc<Orchestrator>,
        wake_signal: CronWakeSignal,
        user_store: Arc<UserStore>,
        user_tool_manager: Arc<UserToolManager>,
        skills: Arc<RwLock<SkillRegistry>>,
    ) -> Self {
        Self {
            config,
            storage,
            orchestrator,
            wake_signal,
            user_store,
            user_tool_manager,
            skills,
        }
    }

    fn start_lease_heartbeat(&self, job: &CronJobRecord) -> Option<CronLeaseHeartbeat> {
        let runner_id = job.runner_id.clone()?;
        let run_token = job.run_token.clone()?;
        let heartbeat_ms = effective_cron_lease_heartbeat_ms(&self.config);
        let ttl_ms = effective_cron_lease_ttl_ms(&self.config);
        let storage = self.storage.clone();
        let user_id = job.user_id.clone();
        let job_id = job.job_id.clone();
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let task = long_task::spawn("cron.lease.heartbeat", async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = sleep(Duration::from_millis(heartbeat_ms)) => {
                        let heartbeat_at = now_ts();
                        let next_lease_expires_at = heartbeat_at + ttl_ms as f64 / 1000.0;
                        let storage = storage.clone();
                        let user_id = user_id.clone();
                        let job_id = job_id.clone();
                        let runner_id = runner_id.clone();
                        let run_token = run_token.clone();
                        let renewed = run_cron_db("cron.lease.renew", move || {
                            storage.renew_cron_job_lease(
                                &user_id,
                                &job_id,
                                &runner_id,
                                &run_token,
                                heartbeat_at,
                                next_lease_expires_at,
                            )
                        })
                        .await;
                        match renewed {
                            Ok(true) => {}
                            Ok(false) => break,
                            Err(err) => {
                                error!("failed to renew cron job lease: {err}");
                                break;
                            }
                        }
                    }
                }
            }
        });
        Some(CronLeaseHeartbeat {
            stop_tx: Some(stop_tx),
            task,
        })
    }

    async fn execute_job(&self, job: CronJobRecord, trigger: &str) {
        let started = Instant::now();
        let start_ts = now_ts();
        let mut status = "ok".to_string();
        let mut summary = None;
        let mut error_msg = None;
        let lease_heartbeat = self.start_lease_heartbeat(&job);
        let message = extract_payload_message(Some(&job.payload));
        let message = match message {
            Some(text) => text,
            None => {
                status = "skipped".to_string();
                error_msg = Some("payload.message is empty".to_string());
                self.finish_job(
                    job,
                    trigger,
                    &status,
                    &summary,
                    &error_msg,
                    start_ts,
                    started,
                    lease_heartbeat,
                )
                .await;
                return;
            }
        };

        let routing = match self.resolve_session_routing(&job) {
            Ok(routing) => routing,
            Err(err) => {
                status = "error".to_string();
                error_msg = Some(format!("resolve session routing failed: {err}"));
                self.finish_job(
                    job,
                    trigger,
                    &status,
                    &summary,
                    &error_msg,
                    start_ts,
                    started,
                    lease_heartbeat,
                )
                .await;
                return;
            }
        };
        let is_isolated = routing.parent_session_id.is_some();

        let run_result = self
            .run_request_when_idle(
                &job.user_id,
                &routing.run_session_id,
                job.agent_id.as_deref(),
                &message,
                routing.parent_session_id.as_deref(),
            )
            .await;

        match run_result {
            Ok(response) => {
                summary = Some(truncate_text(&response.answer, SUMMARY_MAX_CHARS));
                if is_isolated {
                    let deliver_result = self
                        .publish_isolated_result(
                            &job,
                            &routing,
                            &response.answer,
                            started.elapsed().as_secs_f64(),
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

        // Execution history links to the real child thread for isolated runs.
        // The stored job binding is reloaded by finish_job and stays unchanged.
        let mut executed_job = job;
        executed_job.session_id = routing.run_session_id;
        self.finish_job(
            executed_job,
            trigger,
            &status,
            &summary,
            &error_msg,
            start_ts,
            started,
            lease_heartbeat,
        )
        .await;
    }

    fn resolve_session_routing(&self, job: &CronJobRecord) -> Result<CronSessionRouting> {
        resolve_cron_session_routing(self.storage.as_ref(), job)
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
        let runtime = self
            .orchestrator
            .task_runtime
            .read()
            .upgrade()
            .ok_or_else(|| anyhow!("task runtime is unavailable"))?;
        // One admission, one durable identity. Busy threads enter the normal
        // cancellable queue instead of manufacturing a rejected turn per retry.
        match runtime.submit_user_request(request).await? {
            crate::services::runtime::thread::ThreadSubmitOutcome::Run(request, lease) => {
                let _lease = lease;
                self.run_stream_request(*request).await
            }
            crate::services::runtime::thread::ThreadSubmitOutcome::Queued(info) => loop {
                let db = self.storage.clone();
                let id = info.task_id.clone();
                let task = run_cron_db("cron.wait.task", move || db.get_agent_task(&id))
                    .await?
                    .ok_or_else(|| anyhow!("scheduled task disappeared"))?;
                match task.status.as_str() {
                    "success" => {
                        let turn = task
                            .request_payload
                            .pointer("/config_overrides/__thread_log_turn_id")
                            .and_then(Value::as_str)
                            .ok_or_else(|| anyhow!("missing scheduled turn identity"))?;
                        let db = self.storage.clone();
                        let owner = user_id.to_string();
                        let thread = session_id.to_string();
                        let turn = turn.to_string();
                        let answer = run_cron_db("cron.wait.result", move || {
                            read_cron_turn_answer(db.as_ref(), &owner, &thread, &turn)
                        })
                        .await?;
                        return Ok(crate::schemas::WunderResponse {
                            session_id: session_id.to_string(),
                            answer,
                            usage: None,
                            stop_reason: None,
                            uid: None,
                            a2ui: None,
                        });
                    }
                    "failed" | "dead" | "cancelled" => {
                        return Err(anyhow!(
                            "scheduled task {}: {}",
                            task.status,
                            task.last_error.unwrap_or_default()
                        ))
                    }
                    _ => {
                        sleep(Duration::from_millis(
                            self.config.cron.idle_retry_ms.max(200),
                        ))
                        .await
                    }
                }
            },
        }
    }

    async fn publish_isolated_result(
        &self,
        job: &CronJobRecord,
        routing: &CronSessionRouting,
        answer: &str,
        elapsed_s: f64,
    ) -> Result<()> {
        let accepted = self
            .orchestrator
            .committer
            .accept_turn(
                &job.user_id,
                &routing.deliver_session_id,
                &json!({"role":"user", "content":job.payload["message"],
                "client_message_id":format!("cron-result:{}", routing.run_session_id),
                "meta":{"type":"scheduled_result", "job_id":job.job_id,
                    "run_session_id":routing.run_session_id}}),
            )
            .await?;
        let turn = accepted["turn_id"]
            .as_str()
            .ok_or_else(|| anyhow!("missing result turn"))?;
        let copied = self
            .copy_isolated_execution_items(job, routing, turn, elapsed_s)
            .await?;
        if !copied {
            self.orchestrator
                .committer
                .commit_item(
                    &job.user_id,
                    &json!({
                        "session_id":routing.deliver_session_id, "turn_id":turn,
                        "item_id":format!("{turn}:text-0"), "model_round":0,
                        "kind":"assistant_message", "role":"assistant", "status":"completed",
                        "visibility":"user", "content":answer,
                        "meta":{"type":"scheduled_result", "job_id":job.job_id,
                            "run_session_id":routing.run_session_id,
                            "message_stats":{"interaction_duration_s":elapsed_s}}
                    }),
                )
                .await?;
        }
        self.orchestrator
            .committer
            .update_turn(
                &job.user_id,
                &routing.deliver_session_id,
                turn,
                "completed",
                "",
                &json!({"source":"scheduled_result"}),
            )
            .await?;
        Ok(())
    }

    /// Isolated runs execute in a private durable thread, but their visible
    /// execution belongs to the scheduled-result turn in the bound thread.
    /// Copy the durable workflow items in order so the new bubble contains the
    /// model rounds and tool calls instead of only a synthetic final sentence.
    async fn copy_isolated_execution_items(
        &self,
        job: &CronJobRecord,
        routing: &CronSessionRouting,
        destination_turn: &str,
        elapsed_s: f64,
    ) -> Result<bool> {
        let source_turns =
            self.storage
                .list_thread_turns(&job.user_id, &routing.run_session_id, None, 4)?;
        let Some(source_turn) = source_turns.first() else {
            return Ok(false);
        };
        let source_turn_id = source_turn
            .get("turn_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if source_turn_id.is_empty() {
            return Ok(false);
        }
        let page = self
            .storage
            .get_thread_turn(
                &job.user_id,
                &routing.run_session_id,
                source_turn_id,
                -1,
                100,
                true,
            )?
            .ok_or_else(|| anyhow!("isolated scheduled turn disappeared"))?;
        let mut copied = false;
        for source in page["items"].as_array().into_iter().flatten() {
            let kind = source
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if matches!(kind, "user_message" | "terminal") {
                continue;
            }
            if !matches!(
                kind,
                "assistant_message"
                    | "tool_call"
                    | "tool_message"
                    | "progress"
                    | "model_usage"
                    | "compaction"
                    | "plan"
            ) {
                continue;
            }
            let Some(source_item_id) = source.get("item_id").and_then(Value::as_str) else {
                continue;
            };
            let suffix = source_item_id
                .split_once(':')
                .map(|(_, suffix)| suffix)
                .unwrap_or(source_item_id);
            let mut payload = source.get("payload").cloned().unwrap_or_else(|| json!({}));
            let Some(map) = payload.as_object_mut() else {
                continue;
            };
            map.insert(
                "session_id".into(),
                Value::String(routing.deliver_session_id.clone()),
            );
            map.insert(
                "turn_id".into(),
                Value::String(destination_turn.to_string()),
            );
            map.insert(
                "item_id".into(),
                Value::String(format!("{destination_turn}:cron:{suffix}")),
            );
            map.insert("kind".into(), Value::String(kind.to_string()));
            if let Some(status) = source.get("status").and_then(Value::as_str) {
                map.insert("status".into(), Value::String(status.to_string()));
            }
            if kind == "assistant_message" {
                let meta = map.entry("meta").or_insert_with(|| json!({}));
                if let Some(meta) = meta.as_object_mut() {
                    meta.insert("type".into(), Value::String("scheduled_result".into()));
                    meta.insert("job_id".into(), Value::String(job.job_id.clone()));
                    meta.insert(
                        "run_session_id".into(),
                        Value::String(routing.run_session_id.clone()),
                    );
                    if !meta.contains_key("message_stats") {
                        meta.insert(
                            "message_stats".into(),
                            json!({"interaction_duration_s": elapsed_s}),
                        );
                    }
                }
            }
            self.orchestrator
                .committer
                .commit_item(&job.user_id, &payload)
                .await?;
            copied = true;
        }
        Ok(copied)
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
        let parent = parent_session_id
            .map(|id| self.user_store.get_chat_session(&user.user_id, id))
            .transpose()?
            .flatten();
        let mut record = self
            .user_store
            .get_chat_session(&user.user_id, cleaned_session)?
            .unwrap_or_else(|| ChatSessionRecord {
                session_id: cleaned_session.to_string(),
                user_id: user.user_id.clone(),
                title: DEFAULT_SESSION_TITLE.to_string(),
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
                last_message_at: now,
                agent_id: agent_id.map(|value| value.to_string()),
                tool_overrides: parent
                    .as_ref()
                    .map(|p| p.tool_overrides.clone())
                    .unwrap_or_default(),
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
        let agent_record = resolve_agent_record(
            &self.user_store,
            self.storage.as_ref(),
            &user,
            record.agent_id.as_deref(),
        )?;
        self.user_store.upsert_chat_session(&record)?;
        let user_context = self.build_user_tool_context(&user.user_id).await;
        let mut allowed = compute_allowed_tool_names(&user, &user_context);
        let defaults = resolve_agent_tool_defaults(Some(&agent_record));
        let overrides = self
            .orchestrator
            .resolve_frozen_session_tool_overrides(&record, Some(&agent_record))
            .await;
        allowed = apply_tool_overrides(allowed, &overrides, &defaults);
        let tool_names = finalize_tool_names(allowed);
        let agent_prompt = Some(&agent_record)
            .map(|record| record.system_prompt.trim().to_string())
            .filter(|value| !value.is_empty());
        let preview_skill = agent_record.preview_skill;
        let approval_mode = crate::services::user_agent_presets::normalize_agent_approval_mode(
            Some(&agent_record.approval_mode),
        );

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
            client_message_id: None,
            tool_names,
            skip_tool_calls: false,
            stream: true,
            session_id: Some(cleaned_session.to_string()),
            agent_id: record.agent_id.clone(),
            workspace_container_id: Some(agent_record.sandbox_container_id),
            model_name: resolve_chat_model_name(&self.config, Some(&agent_record)),
            language: None,
            config_overrides: Some(json!({"security": {"approval_mode": approval_mode}})),
            agent_prompt,
            preview_skill,
            attachments: None,
            allow_queue: true,
            is_admin: UserStore::is_admin(&user),
            enforce_runtime_queue: true,
            approval_tx: None,
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
            org_units: self.user_store.list_org_units().unwrap_or_default(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn finish_job(
        &self,
        job: CronJobRecord,
        trigger: &str,
        status: &str,
        summary: &Option<String>,
        error_msg: &Option<String>,
        start_ts: f64,
        started: Instant,
        lease_heartbeat: Option<CronLeaseHeartbeat>,
    ) {
        if let Some(lease_heartbeat) = lease_heartbeat {
            lease_heartbeat.stop().await;
        }
        let duration_ms = started.elapsed().as_millis() as i64;
        let now = now_ts();
        let storage = self.storage.clone();
        let job_clone = job.clone();
        let trigger = trigger.to_string();
        let status = status.to_string();
        let summary = summary.clone();
        let error_msg = error_msg.clone();
        let max_consecutive_failures = self.config.cron.max_consecutive_failures.max(1);
        match run_cron_db("cron.runtime.finish_job", move || {
            persist_cron_run_and_update_job_with_limits(
                storage.as_ref(),
                job_clone,
                trigger,
                status,
                summary,
                error_msg,
                start_ts,
                duration_ms,
                now,
                max_consecutive_failures,
            )
        })
        .await
        {
            Ok(()) => {}
            Err(err) => {
                error!("failed to write cron run: {err}");
            }
        }
        self.wake_signal.notify();
    }
}

#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
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
    persist_cron_run_and_update_job_with_limits(
        storage,
        job,
        trigger,
        status,
        summary,
        error_msg,
        start_ts,
        duration_ms,
        now,
        DEFAULT_MAX_CONSECUTIVE_FAILURES,
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_cron_run_and_update_job_with_limits(
    storage: &dyn StorageBackend,
    job: CronJobRecord,
    trigger: String,
    status: String,
    summary: Option<String>,
    error_msg: Option<String>,
    start_ts: f64,
    duration_ms: i64,
    now: f64,
    max_consecutive_failures: usize,
) -> Result<()> {
    let Some(mut record) = storage.get_cron_job(&job.user_id, &job.job_id)? else {
        return Ok(());
    };
    if !cron_job_matches_lease(&record, job.runner_id.as_deref(), job.run_token.as_deref()) {
        return Ok(());
    }
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
    let is_ok = status == "ok";
    let is_error = status == "error";
    clear_cron_job_lease(&mut record);
    record.last_run_at = Some(start_ts);
    record.last_status = Some(status);
    record.last_error = error_msg;
    record.updated_at = now;
    if is_ok {
        record.consecutive_failures = 0;
        record.auto_disabled_reason = None;
    } else if is_error {
        record.consecutive_failures = record.consecutive_failures.saturating_add(1);
    }
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
    if record.schedule_kind.eq_ignore_ascii_case("at") {
        record.enabled = false;
        if !is_ok {
            let reason = record
                .last_error
                .as_deref()
                .unwrap_or("one-shot task failed");
            record.auto_disabled_reason = Some(truncate_text(
                &format!("one-shot disabled: {reason}"),
                AUTO_DISABLED_REASON_MAX_CHARS,
            ));
        }
    }
    let max_consecutive_failures = max_consecutive_failures.max(1) as i64;
    if is_error && record.consecutive_failures >= max_consecutive_failures {
        record.enabled = false;
        record.next_run_at = None;
        let reason = record.last_error.as_deref().unwrap_or("unknown error");
        let reason = truncate_text(
            &format!(
                "auto disabled after {max_consecutive_failures} consecutive failures: {reason}"
            ),
            AUTO_DISABLED_REASON_MAX_CHARS,
        );
        record.auto_disabled_reason = Some(reason);
    } else if is_error && record.enabled {
        let backoff_ms = compute_error_backoff_ms(record.consecutive_failures);
        let backoff_next = now + backoff_ms as f64 / 1000.0;
        record.next_run_at = Some(
            record
                .next_run_at
                .map(|next_run_at| next_run_at.max(backoff_next))
                .unwrap_or(backoff_next),
        );
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
    pub schedule_text: Option<String>,
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
    wake_signal: CronWakeSignal,
    runner_id: String,
    user_store: Arc<UserStore>,
    user_tool_manager: Arc<UserToolManager>,
    skills: Arc<RwLock<SkillRegistry>>,
}

impl CronScheduler {
    pub fn new(
        config_store: ConfigStore,
        storage: Arc<dyn StorageBackend>,
        orchestrator: Arc<Orchestrator>,
        wake_signal: CronWakeSignal,
        user_store: Arc<UserStore>,
        user_tool_manager: Arc<UserToolManager>,
        skills: Arc<RwLock<SkillRegistry>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config_store,
            storage,
            orchestrator,
            wake_signal,
            runner_id: format!("scheduler_{}", Uuid::new_v4().simple()),
            user_store,
            user_tool_manager,
            skills,
        })
    }

    pub fn wake_signal(&self) -> CronWakeSignal {
        self.wake_signal.clone()
    }

    pub fn wake(&self) {
        self.wake_signal.notify();
    }

    pub fn start(self: &Arc<Self>) {
        let scheduler = Arc::clone(self);
        long_task::spawn("cron.scheduler.loop", async move {
            scheduler.run_loop().await;
        });
    }

    async fn run_loop(self: Arc<Self>) {
        loop {
            let config = self.config_store.get().await;
            let cron_cfg = config.cron.clone();
            let wake_signal = self.wake_signal.clone();
            if !cron_cfg.enabled {
                tokio::select! {
                    _ = sleep(Duration::from_millis(cron_cfg.max_idle_sleep_ms.max(500))) => {
                        runtime_metrics::record_loop_tick("cron.scheduler.loop", "disabled_sleep");
                    }
                    _ = wake_signal.wait() => {
                        runtime_metrics::record_loop_tick("cron.scheduler.loop", "disabled_wake");
                    }
                }
                continue;
            }
            let now = now_ts();
            let running = match self.count_running_jobs(now).await {
                Ok(running) => running,
                Err(err) => {
                    error!(error = %err, "cron scheduler could not read running jobs");
                    sleep(Duration::from_millis(cron_cfg.poll_interval_ms.max(500))).await;
                    continue;
                }
            };
            let max_runs = cron_cfg.max_concurrent_runs.max(1) as i64;
            let capacity = (max_runs - running).max(0);
            if capacity > 0 {
                let jobs = self
                    .claim_due_jobs(now, capacity, cron_lease_expires_at(&config, now))
                    .await
                    .unwrap_or_else(|err| {
                        error!(error = %err, "cron scheduler could not claim due jobs");
                        Vec::new()
                    });
                for job in jobs {
                    let scheduler = Arc::clone(&self);
                    long_task::spawn("cron.scheduler.execute_job", async move {
                        scheduler.execute_job(job, "timer").await;
                    });
                }
            }
            let next = self.get_next_cron_run_at(now).await.unwrap_or_else(|err| {
                error!(error = %err, "cron scheduler could not read next run time");
                None
            });
            let sleep_ms = compute_scheduler_sleep_ms(
                now,
                next,
                cron_cfg.poll_interval_ms,
                cron_cfg.max_idle_sleep_ms,
            );
            tokio::select! {
                _ = sleep(Duration::from_millis(sleep_ms)) => {
                    runtime_metrics::record_loop_tick("cron.scheduler.loop", "sleep");
                }
                _ = wake_signal.wait() => {
                    runtime_metrics::record_loop_tick("cron.scheduler.loop", "wake");
                }
            }
        }
    }

    async fn count_running_jobs(&self, now: f64) -> Result<i64> {
        let storage = self.storage.clone();
        let count = run_cron_db("cron.scheduler.count_running", move || {
            storage.count_running_cron_jobs(now)
        })
        .await?;
        Ok(count)
    }

    async fn claim_due_jobs(
        &self,
        now: f64,
        limit: i64,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>> {
        let storage = self.storage.clone();
        let runner_id = self.runner_id.clone();
        let jobs = run_cron_db("cron.scheduler.claim_due_jobs", move || {
            storage.claim_due_cron_jobs(now, limit, &runner_id, lease_expires_at)
        })
        .await?;
        Ok(jobs)
    }

    async fn get_next_cron_run_at(&self, now: f64) -> Result<Option<f64>> {
        let storage = self.storage.clone();
        let next = run_cron_db("cron.scheduler.next_run_at", move || {
            storage.get_next_cron_run_at(now)
        })
        .await?;
        Ok(next)
    }

    async fn execute_job(&self, job: CronJobRecord, trigger: &str) {
        let runtime = CronRuntime::from_scheduler(self).await;
        runtime.execute_job(job, trigger).await;
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::*;
    use serde_json::json;

    #[test]
    fn cron_partial_config_keeps_scheduler_enabled() {
        let config: crate::config::CronConfig =
            serde_json::from_value(json!({"max_concurrent_runs": 2})).unwrap();
        assert!(config.enabled);
        let disabled: crate::config::CronConfig =
            serde_json::from_value(json!({"enabled": false})).unwrap();
        assert!(!disabled.enabled);
    }

    fn now_ts_test() -> f64 {
        chrono::Utc::now().timestamp_millis() as f64 / 1000.0
    }

    #[test]
    fn cron_routing_stays_bound_with_multiple_agent_threads() {
        let dir = tempfile::tempdir().unwrap();
        let db = SqliteStorage::new(dir.path().join("routing.db").to_string_lossy().into_owned());
        let now = now_ts_test();
        db.upsert_chat_session(&build_chat_session("bound", "agent_a", now))
            .unwrap();
        db.upsert_chat_session(&build_chat_session("newer", "agent_a", now + 100.0))
            .unwrap();
        let main =
            super::resolve_cron_session_routing(&db, &build_job("bound", "main", now)).unwrap();
        assert_eq!(main.run_session_id, "bound");
        let isolated =
            super::resolve_cron_session_routing(&db, &build_job("bound", "isolated", now)).unwrap();
        assert_eq!(isolated.deliver_session_id, "bound");
        assert_eq!(isolated.parent_session_id.as_deref(), Some("bound"));
        assert_ne!(isolated.run_session_id, "bound");
        let mut archived = build_chat_session("bound", "agent_a", now);
        archived.status = "archived".into();
        db.upsert_chat_session(&archived).unwrap();
        assert!(
            super::resolve_cron_session_routing(&db, &build_job("bound", "main", now)).is_err()
        );
    }

    async fn runtime_fixture() -> (
        super::CronRuntime,
        std::sync::Arc<crate::state::AppState>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.db_path = dir.path().join("cron.db").to_string_lossy().into_owned();
        config.workspace.root = dir.path().join("workspace").to_string_lossy().into_owned();
        config.agent_queue.enabled = true;
        config.tools.builtin.enabled = ["execute_command", "ptc", "read_file"]
            .into_iter()
            .map(crate::tools::resolve_tool_name)
            .collect();
        let store = crate::config_store::ConfigStore::new(dir.path().join("config.yaml"));
        store
            .update(|current| *current = config.clone())
            .await
            .unwrap();
        let state = std::sync::Arc::new(
            crate::state::AppState::new_with_options(
                store,
                config.clone(),
                crate::state::AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        let runtime = super::CronRuntime::from_parts(
            config,
            state.storage.clone(),
            state.kernel.orchestrator.clone(),
            super::CronWakeSignal::default(),
            state.user_store.clone(),
            state.user_tool_manager.clone(),
            state.skills.clone(),
        );
        state
            .storage
            .upsert_chat_session(&build_chat_session("bound", "agent_a", now_ts_test()))
            .unwrap();
        let mut agent = crate::user_store::build_default_agent_record_from_storage(
            state.storage.as_ref(),
            "cron_user",
        )
        .unwrap();
        agent.agent_id = "agent_a".into();
        state.user_store.upsert_user_agent(&agent).unwrap();
        (runtime, state, dir)
    }

    #[tokio::test]
    async fn cron_default_agent_inherits_approval_and_tools_without_cross_user_lookup() {
        let (runtime, state, _dir) = runtime_fixture().await;
        let mut agent = crate::user_store::build_default_agent_record_from_storage(
            state.storage.as_ref(),
            "cron_user",
        )
        .unwrap();
        agent.approval_mode = "full_auto".into();
        agent.tool_names = vec!["execute_command".into(), "ptc".into()];
        agent.declared_tool_names = agent.tool_names.clone();
        agent.sandbox_container_id = 2;
        let snapshot =
            crate::services::default_agent_protocol::default_agent_config_from_record(&agent);
        state
            .user_store
            .set_meta(
                "default_agent:cron_user",
                &serde_json::to_string(&snapshot).unwrap(),
            )
            .unwrap();
        let mut other = snapshot;
        other.approval_mode = "suggest".into();
        state
            .user_store
            .set_meta(
                "default_agent:other_fixture_user",
                &serde_json::to_string(&other).unwrap(),
            )
            .unwrap();
        let mut bound = build_chat_session("default-bound", "__default__", now_ts_test());
        bound.tool_overrides.clear();
        state.storage.upsert_chat_session(&bound).unwrap();
        let user = super::build_virtual_user("cron_user");
        let interactive = crate::api::chat::build_chat_request(
            &state,
            &user,
            "default-bound",
            "fixture".into(),
            None,
            true,
            None,
            crate::api::chat::ChatRequestOverrides {
                tool_call_mode: None,
                approval_mode: None,
                reasoning_effort: None,
            },
        )
        .await
        .unwrap();
        for alias in [None, Some("__default__"), Some("default")] {
            let request = runtime
                .build_request("cron_user", "fixture-child", alias, "fixture", None)
                .await
                .unwrap();
            assert_eq!(request.tool_names, interactive.tool_names);
            assert_eq!(request.config_overrides, interactive.config_overrides);
            assert_eq!(
                request.config_overrides.as_ref().unwrap()["security"]["approval_mode"],
                "full_auto"
            );
            assert_eq!(request.workspace_container_id, Some(2));
            let mut effective = runtime.config.clone();
            effective.security.approval_mode = request.config_overrides.as_ref().unwrap()
                ["security"]["approval_mode"]
                .as_str()
                .map(str::to_string);
            for tool in ["execute_command", "ptc"] {
                let tool = crate::tools::resolve_tool_name(tool);
                assert!(
                    request
                        .tool_names
                        .iter()
                        .any(|name| crate::tools::resolve_tool_name(name) == tool),
                    "missing {tool}"
                );
                let decision = crate::exec_policy::evaluate_tool_call(
                    &effective,
                    &tool,
                    &json!({"content":"echo fixture"}),
                    Some("fixture-child"),
                    Some("cron_user"),
                );
                assert!(
                    decision.is_none_or(|decision| decision.allowed && !decision.requires_approval)
                );
            }
        }
    }

    #[tokio::test]
    async fn cron_custom_agent_preserves_restrictions_and_rejects_missing_agent() {
        let (runtime, state, _dir) = runtime_fixture().await;
        let mut agent = state
            .user_store
            .get_user_agent_by_id("agent_a")
            .unwrap()
            .unwrap();
        agent.approval_mode = "suggest".into();
        agent.tool_names = vec!["execute_command".into(), "read_file".into()];
        agent.declared_tool_names = agent.tool_names.clone();
        state.user_store.upsert_user_agent(&agent).unwrap();
        let mut parent = build_chat_session("bound", "agent_a", now_ts_test());
        parent.tool_overrides = vec!["read_file".into()];
        state.storage.upsert_chat_session(&parent).unwrap();
        let request = runtime
            .build_request(
                "cron_user",
                "restricted-child",
                Some("agent_a"),
                "fixture",
                Some("bound"),
            )
            .await
            .unwrap();
        assert_eq!(
            request
                .tool_names
                .iter()
                .map(|name| crate::tools::resolve_tool_name(name))
                .collect::<Vec<_>>(),
            vec![crate::tools::resolve_tool_name("read_file")]
        );
        assert_eq!(
            request.config_overrides.unwrap()["security"]["approval_mode"],
            "suggest"
        );
        parent.tool_overrides = vec!["__no_tools__".into()];
        state.storage.upsert_chat_session(&parent).unwrap();
        let request = runtime
            .build_request(
                "cron_user",
                "no-tools-child",
                Some("agent_a"),
                "fixture",
                Some("bound"),
            )
            .await
            .unwrap();
        assert_eq!(request.tool_names, vec!["__no_tools__"]);
        // Explicit user restrictions still bound the inherited agent tool set.
        state
            .user_store
            .set_user_tool_access(
                "cron_user",
                Some(&vec![crate::tools::resolve_tool_name("read_file")]),
            )
            .unwrap();
        let request = runtime
            .build_request(
                "cron_user",
                "user-restricted-child",
                Some("agent_a"),
                "fixture",
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            request
                .tool_names
                .iter()
                .map(|name| crate::tools::resolve_tool_name(name))
                .collect::<Vec<_>>(),
            vec![crate::tools::resolve_tool_name("read_file")]
        );
        // A selected agent owned by someone else must not fall back to all tools.
        agent.user_id = "other_fixture_user".into();
        state.user_store.upsert_user_agent(&agent).unwrap();
        assert!(runtime
            .build_request(
                "cron_user",
                "denied-child",
                Some("agent_a"),
                "fixture",
                None
            )
            .await
            .is_err());
        assert!(runtime
            .build_request(
                "cron_user",
                "missing-child",
                Some("missing-agent"),
                "fixture",
                None
            )
            .await
            .is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cron_real_loop_claims_due_job_and_persists_terminal_failure() {
        let (_runtime, state, _dir) = runtime_fixture().await;
        let now = now_ts_test();
        let mut job = build_job("missing-thread", "isolated", now);
        job.schedule_kind = "at".into();
        job.schedule_at = super::format_ts(Some(now + 0.15));
        job.schedule_every_ms = None;
        job.next_run_at = Some(now + 0.15);
        state.storage.upsert_cron_job(&job).unwrap();
        let scheduler = state.control.cron.clone();
        let task = tokio::spawn(scheduler.run_loop());
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let stored = state
                    .storage
                    .get_cron_job("cron_user", &job.job_id)
                    .unwrap()
                    .unwrap();
                if stored.last_run_at.is_some() {
                    break stored;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await;
        task.abort();
        let stored =
            result.expect("actual scheduler must execute a due task without a service or model");
        assert_eq!(stored.last_status.as_deref(), Some("error"));
        assert!(stored.last_error.as_deref().unwrap().contains("routing"));
        assert!(stored.running_at.is_none());
        let runs = state
            .storage
            .list_cron_runs("cron_user", &job.job_id, 10)
            .unwrap();
        assert_eq!(runs.len(), 1);
    }

    #[tokio::test]
    async fn cron_disabled_cannot_acknowledge_a_new_schedule() {
        let (mut runtime, _state, _dir) = runtime_fixture().await;
        runtime.config.cron.enabled = false;
        let result = super::handle_cron_action(
            runtime.config,
            runtime.storage,
            Some(runtime.orchestrator),
            Some(runtime.wake_signal),
            runtime.user_store,
            runtime.user_tool_manager,
            runtime.skills,
            "cron_user",
            Some("bound"),
            Some("agent_a"),
            super::CronActionRequest {
                action: "add".into(),
                job: None,
            },
        )
        .await;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cron.enabled=false"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cron_busy_thread_queues_once_and_cancel_settles_only_scheduled_turn() {
        let (runtime, state, _dir) = runtime_fixture().await;
        let old = state
            .storage
            .accept_thread_turn("cron_user", "bound", &json!({"content":"existing task"}))
            .unwrap();
        state
            .storage
            .update_thread_turn(
                "cron_user",
                "bound",
                old["turn_id"].as_str().unwrap(),
                "running",
                "",
                &json!({}),
            )
            .unwrap();
        state
            .storage
            .try_acquire_session_lock("bound", "cron_user", "agent_a", 60.0, 10)
            .unwrap();
        let task = tokio::spawn(async move {
            runtime
                .run_request_when_idle(
                    "cron_user",
                    "bound",
                    Some("agent_a"),
                    "scheduled fixture",
                    None,
                )
                .await
        });
        let queued = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(task) = state
                    .user_store
                    .list_agent_tasks_by_thread("thread_bound", None, 10)
                    .unwrap()
                    .pop()
                {
                    break task;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(queued.status, "pending");
        assert_eq!(
            state
                .storage
                .list_thread_turns("cron_user", "bound", None, 10)
                .unwrap()
                .len(),
            2
        );
        state
            .kernel
            .thread_runtime
            .cancel_task(&queued.task_id)
            .await
            .unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        let turns = state
            .storage
            .list_thread_turns("cron_user", "bound", None, 10)
            .unwrap();
        assert_eq!(turns[0]["status"], "cancelled");
        assert_eq!(turns[1]["status"], "running");
        assert_eq!(turns[1]["turn_id"], old["turn_id"]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cron_isolated_result_is_idempotent_completed_pair_without_model_execution() {
        let (runtime, state, _dir) = runtime_fixture().await;
        let job = build_job("bound", "isolated", now_ts_test());
        let routing = runtime.resolve_session_routing(&job).unwrap();
        // No model configured: publication must still work and never acquire a model/session lock.
        runtime
            .publish_isolated_result(&job, &routing, "fixture result", 0.2)
            .await
            .unwrap();
        runtime
            .publish_isolated_result(&job, &routing, "fixture result", 0.2)
            .await
            .unwrap();
        let turns = state
            .storage
            .list_thread_turns("cron_user", "bound", None, 10)
            .unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0]["status"], "completed");
        let turn = turns[0]["turn_id"].as_str().unwrap();
        assert_eq!(
            super::read_cron_turn_answer(state.storage.as_ref(), "cron_user", "bound", turn)
                .unwrap(),
            "fixture result"
        );
        let page = state
            .storage
            .get_thread_turn("cron_user", "bound", turn, -1, 100, false)
            .unwrap()
            .unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn isolated_result_copies_child_workflow_into_delivery_turn() {
        let (runtime, state, _dir) = runtime_fixture().await;
        let job = build_job("bound", "isolated", now_ts_test());
        let routing = runtime.resolve_session_routing(&job).unwrap();
        state
            .storage
            .upsert_chat_session(&build_chat_session(
                &routing.run_session_id,
                "agent_a",
                now_ts_test(),
            ))
            .unwrap();
        let accepted = state
            .storage
            .accept_thread_turn(
                "cron_user",
                &routing.run_session_id,
                &json!({"role":"user","content":"child fixture"}),
            )
            .unwrap();
        let source_turn = accepted["turn_id"].as_str().unwrap();
        state
            .storage
            .commit_thread_item(
                "cron_user",
                &json!({
                    "session_id": routing.run_session_id, "turn_id": source_turn,
                    "item_id": format!("{source_turn}:assistant-1"), "kind": "assistant_message",
                    "role":"assistant", "status":"completed", "visibility":"user",
                    "content":"child answer", "model_round": 1
                }),
            )
            .unwrap();
        state
            .storage
            .commit_thread_item(
                "cron_user",
                &json!({
                    "session_id": routing.run_session_id, "turn_id": source_turn,
                    "item_id": format!("{source_turn}:tool-1"), "kind": "tool_message",
                    "role":"tool", "status":"completed", "visibility":"user",
                    "content":"tool result"
                }),
            )
            .unwrap();
        runtime
            .publish_isolated_result(&job, &routing, "child answer", 0.4)
            .await
            .unwrap();
        let turns = state
            .storage
            .list_thread_turns("cron_user", "bound", None, 10)
            .unwrap();
        let destination = turns.first().unwrap()["turn_id"].as_str().unwrap();
        let page = state
            .storage
            .get_thread_turn("cron_user", "bound", destination, -1, 100, true)
            .unwrap()
            .unwrap();
        let items = page["items"].as_array().unwrap();
        assert!(items.iter().any(|item| item["kind"] == "tool_message"));
        assert!(items
            .iter()
            .any(|item| item.pointer("/payload/content") == Some(&json!("child answer"))));
        assert!(items.iter().all(|item| item["turn_id"] == destination));
    }

    fn build_chat_session(session_id: &str, agent_id: &str, now: f64) -> ChatSessionRecord {
        ChatSessionRecord {
            session_id: session_id.to_string(),
            user_id: "cron_user".to_string(),
            title: session_id.to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: Some(agent_id.to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        }
    }

    fn build_job(session_id: &str, session_target: &str, now: f64) -> CronJobRecord {
        CronJobRecord {
            job_id: format!("job_{session_target}"),
            user_id: "cron_user".to_string(),
            session_id: session_id.to_string(),
            agent_id: Some("agent_a".to_string()),
            name: Some("job".to_string()),
            session_target: session_target.to_string(),
            payload: json!({ "message": "ping" }),
            deliver: None,
            enabled: true,
            delete_after_run: false,
            schedule_kind: "every".to_string(),
            schedule_at: None,
            schedule_every_ms: Some(1000),
            schedule_cron: None,
            schedule_tz: None,
            dedupe_key: None,
            next_run_at: Some(now + 1.0),
            running_at: None,
            runner_id: None,
            run_token: None,
            heartbeat_at: None,
            lease_expires_at: None,
            last_run_at: None,
            last_status: None,
            last_error: None,
            consecutive_failures: 0,
            auto_disabled_reason: None,
            created_at: now,
            updated_at: now,
        }
    }
}
