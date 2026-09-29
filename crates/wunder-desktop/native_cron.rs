use super::NativeDesktop;
use anyhow::{anyhow, Result};

#[derive(Clone, Debug)]
pub struct CronRecord {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: String,
    pub next_run: String,
    pub last_status: String,
    pub last_error: String,
}

impl NativeDesktop {
    pub fn list_cron_jobs(&self) -> Result<Vec<CronRecord>> {
        let mut jobs = self.state().storage.list_cron_jobs(self.user_id(), true)?;
        jobs.sort_by(|a, b| a.updated_at.total_cmp(&b.updated_at).reverse());
        jobs.truncate(200);
        Ok(jobs
            .into_iter()
            .map(|job| CronRecord {
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
                last_status: job.last_status.unwrap_or_else(|| "未运行".into()),
                last_error: job.last_error.unwrap_or_default(),
            })
            .collect())
    }

    pub fn toggle_cron_job(&self, id: &str, enabled: bool) -> Result<()> {
        let mut job = self
            .state()
            .storage
            .get_cron_job(self.user_id(), id)?
            .ok_or_else(|| anyhow!("定时任务不存在"))?;
        job.enabled = enabled;
        job.updated_at = now();
        self.state().storage.upsert_cron_job(&job)?;
        Ok(())
    }

    pub fn delete_cron_job(&self, id: &str) -> Result<()> {
        if self.state().storage.delete_cron_job(self.user_id(), id)? == 0 {
            return Err(anyhow!("定时任务不存在"));
        }
        Ok(())
    }
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs_f64())
        .unwrap_or(0.0)
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
