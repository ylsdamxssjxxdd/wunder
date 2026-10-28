use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use wunder_server::cron::{
    handle_cron_action, list_cron_runs, CronActionRequest, CronJobInput, CronScheduleInput,
};

/// Cap for run records shown per request. Matches the plan's log/run limit.
const CRON_RUNS_LIMIT: i64 = 200;

#[derive(Clone, Debug)]
pub struct CronRecord {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: String,
    pub next_run: String,
    pub last_status: String,
    pub last_error: String,
    pub workspace_id: String,
    pub workspace_name: String,
    /// Prompt message sent on each run.
    pub message: String,
    pub delete_after_run: bool,
    /// Raw schedule fields for editor prefill.
    pub schedule_kind: String,
    pub schedule_at: String,
    pub every_s: i64,
    pub cron_expr: String,
    pub timezone: String,
}

/// Typed secret-free edit input for create and update. The runtime service
/// revalidates every field; the façade only performs upfront trimming.
#[derive(Clone, Debug, Default)]
pub struct NativeCronJobEdit {
    /// Empty to create a new job.
    pub job_id: String,
    pub name: String,
    /// "at" | "every" | "cron"
    pub schedule_kind: String,
    /// Local wall-clock "YYYY-MM-DD HH:MM" for at/every first run.
    pub schedule_at: String,
    /// Seconds for the "every" kind (1..86400).
    pub every_s: i64,
    pub cron_expr: String,
    pub timezone: String,
    pub message: String,
    pub workspace_id: String,
    pub delete_after_run: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct CronRunRecord {
    pub run_id: String,
    pub status: String,
    pub trigger: String,
    pub summary: String,
    pub error: String,
    pub duration_ms: i64,
    pub created_at: String,
}

impl NativeDesktop {
    pub fn list_cron_jobs(&self) -> Result<Vec<CronRecord>> {
        let jobs = self
            .state()
            .storage
            .list_cron_jobs_page(self.user_id(), true, None, 200, 0)?;
        let names = self.cron_workspace_names()?;
        let sessions = jobs
            .iter()
            .map(|job| job.session_id.clone())
            .collect::<Vec<_>>();
        let bindings = self
            .state()
            .storage
            .cron_workspace_bindings(self.user_id(), &sessions)?;
        Ok(jobs
            .into_iter()
            .map(|job| {
                let workspace_id = bindings
                    .get(&job.session_id)
                    .cloned()
                    .flatten()
                    .unwrap_or_default();
                let mut record = cron_record_from(job);
                record.workspace_name = names.get(&workspace_id).cloned().unwrap_or_default();
                record.workspace_id = workspace_id;
                record
            })
            .collect())
    }

    pub fn get_cron_job(&self, id: &str) -> Result<CronRecord> {
        self.state()
            .storage
            .get_cron_job(self.user_id(), id.trim())?
            .ok_or_else(|| anyhow!("定时任务不存在"))
            .and_then(|job| self.cron_record(job, &self.cron_workspace_names()?))
    }

    fn cron_workspace_names(&self) -> Result<std::collections::HashMap<String, String>> {
        Ok(self
            .state()
            .storage
            .list_workspaces(self.user_id())?
            .into_iter()
            .map(|workspace| (workspace.workspace_id, workspace.name))
            .collect())
    }

    fn cron_record(
        &self,
        job: wunder_server::storage::CronJobRecord,
        names: &std::collections::HashMap<String, String>,
    ) -> Result<CronRecord> {
        let workspace_id = self
            .state()
            .storage
            .get_chat_session(self.user_id(), &job.session_id)?
            .and_then(|session| session.workspace_id)
            .unwrap_or_default();
        let mut record = cron_record_from(job);
        record.workspace_name = names.get(&workspace_id).cloned().unwrap_or_default();
        record.workspace_id = workspace_id;
        Ok(record)
    }

    pub fn save_cron_job(&self, edit: &NativeCronJobEdit) -> Result<CronRecord> {
        let name = edit.name.trim();
        if name.is_empty() {
            return Err(anyhow!("任务名称不能为空"));
        }
        let message = edit.message.trim();
        if message.is_empty() {
            return Err(anyhow!("执行消息不能为空"));
        }
        let kind = edit.schedule_kind.trim();
        let at = match kind {
            "at" => Some(local_at_to_rfc3339(&edit.schedule_at)?),
            "every" => {
                let every_s = edit.every_s;
                if !(1..=86_400).contains(&every_s) {
                    return Err(anyhow!("执行间隔必须在 1..86400 秒之间"));
                }
                let first_run = edit.schedule_at.trim();
                (!first_run.is_empty())
                    .then(|| local_at_to_rfc3339(first_run))
                    .transpose()?
            }
            _ => None,
        };
        let tz = edit.timezone.trim();
        let schedule = match kind {
            "at" => CronScheduleInput {
                kind: "at".into(),
                at,
                every_ms: None,
                cron: None,
                tz: None,
            },
            "every" => CronScheduleInput {
                kind: "every".into(),
                at,
                every_ms: Some(edit.every_s * 1000),
                cron: None,
                tz: None,
            },
            "cron" => CronScheduleInput {
                kind: "cron".into(),
                at: None,
                every_ms: None,
                cron: Some(edit.cron_expr.trim().to_string()),
                tz: (!tz.is_empty()).then(|| tz.to_string()),
            },
            other => return Err(anyhow!("不支持的调度类型：{other}")),
        };
        let workspace_id = edit.workspace_id.trim();
        if workspace_id.is_empty() {
            return Err(anyhow!("请选择工作区"));
        }
        self.state()
            .storage
            .get_workspace(self.user_id(), workspace_id)?
            .ok_or_else(|| anyhow!("工作区不存在"))?;
        let job_id = edit.job_id.trim();
        let input = CronJobInput {
            job_id: (!job_id.is_empty()).then(|| job_id.to_string()),
            name: Some(name.to_string()),
            schedule: Some(schedule),
            schedule_text: None,
            session: Some("main".to_string()),
            payload: Some(json!({ "message": message })),
            deliver: None,
            enabled: Some(edit.enabled),
            delete_after_run: Some(edit.delete_after_run),
            dedupe_key: None,
            session_id: None,
            agent_id: None,
            workspace_id: Some(workspace_id.to_string()),
        };
        let action = if job_id.is_empty() { "add" } else { "update" };
        let response = self.cron_action(action, input)?;
        let job_id = response
            .get("job")
            .and_then(|job| job.get("job_id"))
            .and_then(Value::as_str)
            .unwrap_or(job_id)
            .to_string();
        self.get_cron_job(&job_id)
    }

    pub fn toggle_cron_job(&self, id: &str, enabled: bool) -> Result<()> {
        self.cron_action(
            if enabled { "enable" } else { "disable" },
            CronJobInput {
                job_id: Some(id.trim().to_string()),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    /// Queue an immediate manual run through the shared scheduler. Returns
    /// "queued" or "running" (the job already has an active run).
    pub fn run_cron_job_now(&self, id: &str) -> Result<String> {
        let input = CronJobInput {
            job_id: Some(id.trim().to_string()),
            ..Default::default()
        };
        let response = self.cron_action("run", input)?;
        Ok(response
            .get("queued")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            .then(|| "queued".to_string())
            .unwrap_or_else(|| "running".to_string()))
    }

    /// Run records for one job, newest first, capped at 200.
    pub fn list_cron_job_runs(&self, job_id: &str) -> Result<Vec<CronRunRecord>> {
        let cleaned = job_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let storage = self.state().storage.clone();
        let user = self.user_id().to_string();
        let job = cleaned.to_string();
        let result =
            self.runtime
                .block_on(list_cron_runs(storage, &user, &job, None, CRON_RUNS_LIMIT))?;
        let mut runs = Vec::new();
        if let Some(items) = result.get("runs").and_then(Value::as_array) {
            for item in items {
                runs.push(CronRunRecord {
                    run_id: item
                        .get("run_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    status: item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    trigger: item
                        .get("trigger")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    summary: item
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    error: item
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    duration_ms: item
                        .get("duration_ms")
                        .and_then(Value::as_i64)
                        .unwrap_or_default(),
                    created_at: item
                        .get("created_at_text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        Ok(runs)
    }

    pub fn delete_cron_job(&self, id: &str) -> Result<()> {
        self.cron_action(
            "remove",
            CronJobInput {
                job_id: Some(id.trim().to_string()),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    fn cron_action(&self, action: &str, input: CronJobInput) -> Result<Value> {
        let state = self.state();
        let config = self.runtime.block_on(state.config_store.get());
        let response = self.runtime.block_on(handle_cron_action(
            config,
            state.storage.clone(),
            Some(state.kernel.orchestrator.clone()),
            Some(state.control.cron.wake_signal()),
            state.user_store.clone(),
            state.user_tool_manager.clone(),
            state.skills.clone(),
            self.user_id(),
            None,
            None,
            CronActionRequest {
                action: action.to_string(),
                job: Some(input),
            },
        ))?;
        Ok(response)
    }
}

fn cron_record_from(job: wunder_server::storage::CronJobRecord) -> CronRecord {
    CronRecord {
        id: job.job_id,
        name: job.name.unwrap_or_else(|| "未命名任务".into()),
        enabled: job.enabled,
        schedule: format_schedule(
            &job.schedule_kind,
            job.schedule_at.as_deref(),
            job.schedule_every_ms,
            job.schedule_cron.as_deref(),
            job.schedule_tz.as_deref(),
        ),
        next_run: job
            .next_run_at
            .map(format_ts)
            .unwrap_or_else(|| "未计划".into()),
        last_status: if job.running_at.is_some()
            && job.lease_expires_at.is_some_and(|expires| {
                expires > chrono::Utc::now().timestamp_millis() as f64 / 1000.0
            }) {
            "运行中".into()
        } else {
            job.last_status.unwrap_or_else(|| "未运行".into())
        },
        last_error: job.last_error.unwrap_or_default(),
        workspace_id: String::new(),
        workspace_name: String::new(),
        message: job
            .payload
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        delete_after_run: job.delete_after_run,
        schedule_kind: job.schedule_kind,
        schedule_at: job.schedule_at.unwrap_or_default(),
        every_s: job.schedule_every_ms.unwrap_or_default() / 1000,
        cron_expr: job.schedule_cron.unwrap_or_default(),
        timezone: job.schedule_tz.unwrap_or_default(),
    }
}

/// The UI collects local wall-clock text; the runtime parses RFC3339.
fn local_at_to_rfc3339(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("执行时间不能为空"));
    }
    if chrono::DateTime::parse_from_rfc3339(trimmed).is_ok() {
        return Ok(trimmed.to_string());
    }
    let formats = ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M", "%Y/%m/%d %H:%M"];
    for format in formats {
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(trimmed, format) {
            let local = chrono_local_offset();
            return Ok(parsed
                .and_local_timezone(local)
                .single()
                .map(|value| value.to_rfc3339())
                .ok_or_else(|| anyhow!("无法解析执行时间：{trimmed}"))?);
        }
    }
    Err(anyhow!("无法解析执行时间：{trimmed}"))
}

fn chrono_local_offset() -> chrono::FixedOffset {
    *chrono::Local::now().offset()
}

fn format_ts(value: f64) -> String {
    chrono::DateTime::from_timestamp(value as i64, 0)
        .map(|v| v.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "未知".into())
}
fn format_schedule(
    kind: &str,
    at: Option<&str>,
    every: Option<i64>,
    cron: Option<&str>,
    tz: Option<&str>,
) -> String {
    match kind {
        "at" => at.unwrap_or("一次性").into(),
        "every" => format!("每 {} 秒", every.unwrap_or(0) / 1000),
        "cron" => format!(
            "{}{}",
            cron.unwrap_or("cron"),
            tz.map(|v| format!(" · {v}")).unwrap_or_default()
        ),
        _ => kind.into(),
    }
}
