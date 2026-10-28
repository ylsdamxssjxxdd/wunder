use super::SqliteStorage;
use crate::storage::{CronJobRecord, CronJobStats, CronRunRecord, StorageLifecycle};
use anyhow::Result;
use rusqlite::{params, params_from_iter, OptionalExtension, TransactionBehavior};
use std::collections::HashMap;

pub(super) trait SqliteCronStorage {
    fn cron_workspace_bindings_impl(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, Option<String>>>;
    fn compare_and_swap_cron_job_impl(
        &self,
        expected: &CronJobRecord,
        replacement: Option<&CronJobRecord>,
        run: Option<&CronRunRecord>,
    ) -> Result<bool>;
    fn upsert_cron_job_impl(&self, record: &CronJobRecord) -> Result<()>;
    fn get_cron_job_impl(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>>;
    fn get_cron_job_by_dedupe_key_impl(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>>;
    fn list_cron_jobs_page_impl(
        &self,
        user_id: &str,
        include_disabled: bool,
        agent_id: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CronJobRecord>>;
    fn summarize_cron_jobs_impl(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        now: f64,
    ) -> Result<CronJobStats>;
    fn delete_cron_job_impl(&self, user_id: &str, job_id: &str) -> Result<i64>;
    fn delete_cron_jobs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
    fn reset_cron_jobs_running_impl(&self) -> Result<()>;
    fn count_running_cron_jobs_impl(&self, now: f64) -> Result<i64>;
    fn claim_due_cron_jobs_impl(
        &self,
        now: f64,
        limit: i64,
        runner_id: &str,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>>;
    fn renew_cron_job_lease_impl(
        &self,
        user_id: &str,
        job_id: &str,
        runner_id: &str,
        run_token: &str,
        heartbeat_at: f64,
        lease_expires_at: f64,
    ) -> Result<bool>;
    fn insert_cron_run_impl(&self, record: &CronRunRecord) -> Result<()>;
    fn list_cron_runs_impl(
        &self,
        user_id: &str,
        job_id: &str,
        limit: i64,
    ) -> Result<Vec<CronRunRecord>>;
    fn get_next_cron_run_at_impl(&self, now: f64) -> Result<Option<f64>>;
}

impl SqliteCronStorage for SqliteStorage {
    fn cron_workspace_bindings_impl(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<HashMap<String, Option<String>>> {
        self.ensure_initialized()?;
        anyhow::ensure!(
            session_ids.len() <= 201,
            "cron workspace lookup exceeds page limit"
        );
        if session_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.read_connection()?;
        let placeholders = vec!["?"; session_ids.len()].join(",");
        let sql = format!("SELECT session_id, workspace_id FROM chat_sessions WHERE user_id = ? AND session_id IN ({placeholders})");
        let mut statement = conn.prepare(&sql)?;
        let parameters = std::iter::once(user_id).chain(session_ids.iter().map(String::as_str));
        let rows = statement.query_map(params_from_iter(parameters), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }
    fn upsert_cron_job_impl(&self, record: &CronJobRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        write_cron_job(&conn, record)
    }

    fn compare_and_swap_cron_job_impl(
        &self,
        expected: &CronJobRecord,
        replacement: Option<&CronJobRecord>,
        run: Option<&CronRunRecord>,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        anyhow::ensure!(
            replacement.is_none_or(
                |record| record.job_id == expected.job_id && record.user_id == expected.user_id
            ),
            "cron replacement identity mismatch"
        );
        anyhow::ensure!(
            run.is_none_or(
                |record| record.job_id == expected.job_id && record.user_id == expected.user_id
            ),
            "cron run identity mismatch"
        );
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sql = format!(
            "SELECT {} FROM cron_jobs WHERE user_id = ? AND job_id = ?",
            cron_job_select_fields()
        );
        let current = tx
            .query_row(
                &sql,
                params![expected.user_id, expected.job_id],
                map_cron_job_row,
            )
            .optional()?;
        if current.as_ref() != Some(expected) {
            return Ok(false);
        }
        if let Some(record) = replacement {
            write_cron_job(&tx, record)?;
        } else {
            tx.execute(
                "DELETE FROM cron_jobs WHERE user_id = ? AND job_id = ?",
                params![expected.user_id, expected.job_id],
            )?;
        }
        if let Some(record) = run {
            write_cron_run(&tx, record)?;
        }
        tx.commit()?;
        Ok(true)
    }

    fn get_cron_job_impl(&self, user_id: &str, job_id: &str) -> Result<Option<CronJobRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_job = job_id.trim();
        if cleaned_user.is_empty() || cleaned_job.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let sql = format!(
            "SELECT {} FROM cron_jobs WHERE user_id = ? AND job_id = ?",
            cron_job_select_fields()
        );
        let row = conn
            .query_row(&sql, params![cleaned_user, cleaned_job], map_cron_job_row)
            .optional()?;
        Ok(row)
    }

    fn get_cron_job_by_dedupe_key_impl(
        &self,
        user_id: &str,
        dedupe_key: &str,
    ) -> Result<Option<CronJobRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_key = dedupe_key.trim();
        if cleaned_user.is_empty() || cleaned_key.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let sql = format!(
        "SELECT {} FROM cron_jobs WHERE user_id = ? AND dedupe_key = ? ORDER BY updated_at DESC LIMIT 1",
        cron_job_select_fields()
    );
        let row = conn
            .query_row(&sql, params![cleaned_user, cleaned_key], map_cron_job_row)
            .optional()?;
        Ok(row)
    }

    fn list_cron_jobs_page_impl(
        &self,
        user_id: &str,
        include_disabled: bool,
        agent_id: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CronJobRecord>> {
        self.ensure_initialized()?;
        let limit = limit.clamp(1, 201);
        let offset = offset.max(0);
        let enabled = if include_disabled { 1 } else { 0 };
        let conn = self.read_connection()?;
        let sql = format!("SELECT {} FROM cron_jobs WHERE user_id = ?1 AND (?2 = 1 OR enabled = 1) AND (?3 IS NULL OR agent_id = ?3) ORDER BY updated_at DESC, job_id ASC LIMIT ?4 OFFSET ?5", cron_job_select_fields());
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(
            params![user_id.trim(), enabled, agent_id, limit, offset],
            map_cron_job_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn summarize_cron_jobs_impl(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        now: f64,
    ) -> Result<CronJobStats> {
        self.ensure_initialized()?;
        let conn = self.read_connection()?;
        Ok(conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN enabled = 1 THEN 1 ELSE 0 END), 0), COALESCE(SUM(CASE WHEN running_at IS NOT NULL AND lease_expires_at > ?3 THEN 1 ELSE 0 END), 0), MIN(CASE WHEN enabled = 1 THEN next_run_at END) FROM cron_jobs WHERE user_id = ?1 AND (?2 IS NULL OR agent_id = ?2)",
            params![user_id.trim(), agent_id, now],
            |row| Ok(CronJobStats { total: row.get(0)?, enabled: row.get(1)?, running: row.get(2)?, next_run_at: row.get(3)? }),
        )?)
    }

    fn delete_cron_job_impl(&self, user_id: &str, job_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_job = job_id.trim();
        if cleaned_user.is_empty() || cleaned_job.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM cron_jobs WHERE user_id = ? AND job_id = ?",
            params![cleaned_user, cleaned_job],
        )?;
        Ok(affected as i64)
    }

    fn delete_cron_jobs_by_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM cron_jobs WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }

    fn reset_cron_jobs_running_impl(&self) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
        "UPDATE cron_jobs SET running_at = NULL, runner_id = NULL, run_token = NULL, heartbeat_at = NULL, lease_expires_at = NULL WHERE running_at IS NOT NULL OR runner_id IS NOT NULL OR run_token IS NOT NULL OR heartbeat_at IS NOT NULL OR lease_expires_at IS NOT NULL",
        [],
    )?;
        Ok(())
    }

    fn count_running_cron_jobs_impl(&self, now: f64) -> Result<i64> {
        self.ensure_initialized()?;
        let conn = self.read_connection()?;
        let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM cron_jobs WHERE running_at IS NOT NULL AND lease_expires_at IS NOT NULL AND lease_expires_at > ?",
        params![now],
        |row| row.get(0),
    )?;
        Ok(total)
    }

    fn claim_due_cron_jobs_impl(
        &self,
        now: f64,
        limit: i64,
        runner_id: &str,
        lease_expires_at: f64,
    ) -> Result<Vec<CronJobRecord>> {
        self.ensure_initialized()?;
        let limit = limit.max(0);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = {
            let mut stmt = tx.prepare(
            "SELECT job_id FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ? AND (running_at IS NULL OR lease_expires_at IS NULL OR lease_expires_at <= ?) ORDER BY next_run_at ASC LIMIT ?",
        )?;
            let ids = stmt
                .query_map(params![now, now, limit], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            ids
        };
        if ids.is_empty() {
            tx.commit()?;
            return Ok(Vec::new());
        }
        for id in &ids {
            let run_token = uuid::Uuid::new_v4().simple().to_string();
            tx.execute(
            "UPDATE cron_jobs SET running_at = ?, runner_id = ?, run_token = ?, heartbeat_at = ?, lease_expires_at = ?, updated_at = ? WHERE job_id = ?",
            params![now, runner_id, run_token, now, lease_expires_at, now, id],
        )?;
        }
        let placeholders = vec!["?"; ids.len()].join(", ");
        let sql = format!(
            "SELECT {} FROM cron_jobs WHERE job_id IN ({placeholders})",
            cron_job_select_fields()
        );
        let mut output = Vec::new();
        {
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(ids.iter()), map_cron_job_row)?;
            for record in rows.flatten() {
                output.push(record);
            }
        }
        tx.commit()?;
        Ok(output)
    }

    fn renew_cron_job_lease_impl(
        &self,
        user_id: &str,
        job_id: &str,
        runner_id: &str,
        run_token: &str,
        heartbeat_at: f64,
        lease_expires_at: f64,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let affected = conn.execute(
        "UPDATE cron_jobs SET heartbeat_at = ?, lease_expires_at = ?, updated_at = ? WHERE user_id = ? AND job_id = ? AND runner_id = ? AND run_token = ? AND running_at IS NOT NULL AND lease_expires_at IS NOT NULL AND lease_expires_at > ?",
        params![
            heartbeat_at,
            lease_expires_at,
            heartbeat_at,
            user_id.trim(),
            job_id.trim(),
            runner_id,
            run_token,
            heartbeat_at,
        ],
    )?;
        Ok(affected > 0)
    }

    fn insert_cron_run_impl(&self, record: &CronRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        write_cron_run(&conn, record)
    }

    fn list_cron_runs_impl(
        &self,
        user_id: &str,
        job_id: &str,
        limit: i64,
    ) -> Result<Vec<CronRunRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_job = job_id.trim();
        if cleaned_user.is_empty() || cleaned_job.is_empty() {
            return Ok(Vec::new());
        }
        let safe_limit = limit.clamp(1, 200);
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT run_id, job_id, user_id, session_id, agent_id, trigger, status, summary, error, duration_ms, created_at \
             FROM cron_runs WHERE user_id = ? AND job_id = ? ORDER BY created_at DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![cleaned_user, cleaned_job, safe_limit], |row| {
            Ok(CronRunRecord {
                run_id: row.get(0)?,
                job_id: row.get(1)?,
                user_id: row.get(2)?,
                session_id: row.get(3)?,
                agent_id: row.get(4)?,
                trigger: row.get(5)?,
                status: row.get(6)?,
                summary: row.get(7)?,
                error: row.get(8)?,
                duration_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                created_at: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn get_next_cron_run_at_impl(&self, now: f64) -> Result<Option<f64>> {
        self.ensure_initialized()?;
        let conn = self.read_connection()?;
        let value: Option<f64> = conn.query_row(
            "SELECT MIN(CASE WHEN running_at IS NOT NULL AND lease_expires_at > ?1 THEN MAX(next_run_at, lease_expires_at) ELSE next_run_at END) FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL",
            params![now],
            |row| row.get::<_, Option<f64>>(0),
        )?;
        Ok(value)
    }
}

fn cron_job_select_fields() -> &'static str {
    "job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, runner_id, run_token, heartbeat_at, lease_expires_at, last_run_at, last_status, last_error, consecutive_failures, auto_disabled_reason, created_at, updated_at"
}

fn map_cron_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CronJobRecord> {
    let payload_text: Option<String> = row.get(6)?;
    let deliver_text: Option<String> = row.get(7)?;
    let enabled: Option<i64> = row.get(8)?;
    let delete_after: Option<i64> = row.get(9)?;
    Ok(CronJobRecord {
        job_id: row.get(0)?,
        user_id: row.get(1)?,
        session_id: row.get(2)?,
        agent_id: row.get(3)?,
        name: row.get(4)?,
        session_target: row.get(5)?,
        payload: SqliteStorage::json_value_or_null(payload_text),
        deliver: deliver_text.and_then(|value| SqliteStorage::json_from_str(&value)),
        enabled: enabled.unwrap_or(0) != 0,
        delete_after_run: delete_after.unwrap_or(0) != 0,
        schedule_kind: row.get(10)?,
        schedule_at: row.get(11)?,
        schedule_every_ms: row.get(12)?,
        schedule_cron: row.get(13)?,
        schedule_tz: row.get(14)?,
        dedupe_key: row.get(15)?,
        next_run_at: row.get(16)?,
        running_at: row.get(17)?,
        runner_id: row.get(18)?,
        run_token: row.get(19)?,
        heartbeat_at: row.get(20)?,
        lease_expires_at: row.get(21)?,
        last_run_at: row.get(22)?,
        last_status: row.get(23)?,
        last_error: row.get(24)?,
        consecutive_failures: row.get::<_, Option<i64>>(25)?.unwrap_or(0),
        auto_disabled_reason: row.get(26)?,
        created_at: row.get(27)?,
        updated_at: row.get(28)?,
    })
}

fn write_cron_job(tx: &rusqlite::Connection, record: &CronJobRecord) -> Result<()> {
    let payload = SqliteStorage::json_to_string(&record.payload);
    let deliver = record.deliver.as_ref().map(SqliteStorage::json_to_string);
    tx.execute(
        "INSERT INTO cron_jobs (job_id, user_id, session_id, agent_id, name, session_target, payload, deliver, enabled, delete_after_run, schedule_kind, schedule_at, schedule_every_ms, schedule_cron, schedule_tz, dedupe_key, next_run_at, running_at, runner_id, run_token, heartbeat_at, lease_expires_at, last_run_at, last_status, last_error, consecutive_failures, auto_disabled_reason, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(job_id) DO UPDATE SET user_id = excluded.user_id, session_id = excluded.session_id, agent_id = excluded.agent_id, name = excluded.name, session_target = excluded.session_target, payload = excluded.payload, deliver = excluded.deliver, enabled = excluded.enabled, delete_after_run = excluded.delete_after_run, schedule_kind = excluded.schedule_kind, schedule_at = excluded.schedule_at, schedule_every_ms = excluded.schedule_every_ms, schedule_cron = excluded.schedule_cron, schedule_tz = excluded.schedule_tz, dedupe_key = excluded.dedupe_key, next_run_at = excluded.next_run_at, running_at = excluded.running_at, runner_id = excluded.runner_id, run_token = excluded.run_token, heartbeat_at = excluded.heartbeat_at, lease_expires_at = excluded.lease_expires_at, last_run_at = excluded.last_run_at, last_status = excluded.last_status, last_error = excluded.last_error, consecutive_failures = excluded.consecutive_failures, auto_disabled_reason = excluded.auto_disabled_reason, updated_at = excluded.updated_at",
        params![
            record.job_id,
            record.user_id,
            record.session_id,
            record.agent_id,
            record.name,
            record.session_target,
            payload,
            deliver,
            if record.enabled { 1 } else { 0 },
            if record.delete_after_run { 1 } else { 0 },
            record.schedule_kind,
            record.schedule_at,
            record.schedule_every_ms,
            record.schedule_cron,
            record.schedule_tz,
            record.dedupe_key,
            record.next_run_at,
            record.running_at,
            record.runner_id,
            record.run_token,
            record.heartbeat_at,
            record.lease_expires_at,
            record.last_run_at,
            record.last_status,
            record.last_error,
            record.consecutive_failures,
            record.auto_disabled_reason,
            record.created_at,
            record.updated_at
        ],
    )?;
    Ok(())
}

fn write_cron_run(tx: &rusqlite::Connection, record: &CronRunRecord) -> Result<()> {
    tx.execute(
            "INSERT INTO cron_runs (run_id, job_id, user_id, session_id, agent_id, trigger, status, summary, error, duration_ms, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.run_id,
                record.job_id,
                record.user_id,
                record.session_id,
                record.agent_id,
                record.trigger,
                record.status,
                record.summary,
                record.error,
                record.duration_ms,
                record.created_at
            ],
        )?;
    Ok(())
}
