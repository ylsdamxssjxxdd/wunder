use super::SqliteStorage;
use crate::storage::{SessionRunRecord, StorageBackend};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

pub(super) trait SqliteSessionRunStorage {
    fn upsert_session_run_impl(&self, record: &SessionRunRecord) -> Result<()>;
    fn get_session_run_impl(&self, run_id: &str) -> Result<Option<SessionRunRecord>>;
    fn list_session_runs_by_session_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
    fn list_session_runs_by_parent_impl(
        &self,
        user_id: &str,
        parent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
    fn list_session_runs_by_dispatch_impl(
        &self,
        user_id: &str,
        dispatch_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>>;
}

impl SqliteSessionRunStorage for SqliteStorage {
    fn upsert_session_run_impl(&self, record: &SessionRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO session_runs (run_id, session_id, parent_session_id, user_id, dispatch_id, run_kind, requested_by, agent_id, model_name, status, queued_time, \
             started_time, finished_time, elapsed_s, result, error, updated_time, metadata) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(run_id) DO UPDATE SET session_id = excluded.session_id, parent_session_id = excluded.parent_session_id, \
             user_id = excluded.user_id, dispatch_id = excluded.dispatch_id, run_kind = excluded.run_kind, requested_by = excluded.requested_by, \
             agent_id = excluded.agent_id, model_name = excluded.model_name, status = excluded.status, \
             queued_time = excluded.queued_time, started_time = excluded.started_time, finished_time = excluded.finished_time, \
             elapsed_s = excluded.elapsed_s, result = excluded.result, error = excluded.error, updated_time = excluded.updated_time, \
             metadata = excluded.metadata",
            params![
                record.run_id,
                record.session_id,
                record.parent_session_id,
                record.user_id,
                record.dispatch_id,
                record.run_kind,
                record.requested_by,
                record.agent_id,
                record.model_name,
                record.status,
                record.queued_time,
                record.started_time,
                record.finished_time,
                record.elapsed_s,
                record.result,
                record.error,
                record.updated_time,
                metadata
            ],
        )?;
        Ok(())
    }

    fn get_session_run_impl(&self, run_id: &str) -> Result<Option<SessionRunRecord>> {
        self.ensure_initialized()?;
        let cleaned = run_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                &format!(
                    "SELECT {} FROM session_runs WHERE run_id = ?",
                    session_run_select_fields()
                ),
                params![cleaned],
                map_session_run_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_session_runs_by_session_impl(
        &self,
        user_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM session_runs WHERE user_id = ? AND session_id = ? \
             ORDER BY updated_time DESC, queued_time DESC LIMIT ?",
            session_run_select_fields()
        ))?;
        let rows = stmt.query_map(
            params![cleaned_user, cleaned_session, limit],
            map_session_run_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn list_session_runs_by_parent_impl(
        &self,
        user_id: &str,
        parent_session_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_parent = parent_session_id.trim();
        if cleaned_user.is_empty() || cleaned_parent.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM session_runs WHERE user_id = ? AND parent_session_id = ? \
             ORDER BY updated_time DESC, queued_time DESC LIMIT ?",
            session_run_select_fields()
        ))?;
        let rows = stmt.query_map(
            params![cleaned_user, cleaned_parent, limit],
            map_session_run_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn list_session_runs_by_dispatch_impl(
        &self,
        user_id: &str,
        dispatch_id: &str,
        limit: i64,
    ) -> Result<Vec<SessionRunRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_dispatch = dispatch_id.trim();
        if cleaned_user.is_empty() || cleaned_dispatch.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM session_runs WHERE user_id = ? AND dispatch_id = ? \
             ORDER BY updated_time DESC, queued_time DESC LIMIT ?",
            session_run_select_fields()
        ))?;
        let rows = stmt.query_map(
            params![cleaned_user, cleaned_dispatch, limit],
            map_session_run_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }
}

fn session_run_select_fields() -> &'static str {
    "run_id, session_id, parent_session_id, user_id, dispatch_id, run_kind, requested_by, agent_id, model_name, status, queued_time, started_time, finished_time, elapsed_s, result, error, updated_time, metadata"
}

fn map_session_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRunRecord> {
    Ok(SessionRunRecord {
        run_id: row.get(0)?,
        session_id: row.get(1)?,
        parent_session_id: row.get(2)?,
        user_id: row.get(3)?,
        dispatch_id: row.get(4)?,
        run_kind: row.get(5)?,
        requested_by: row.get(6)?,
        agent_id: row.get(7)?,
        model_name: row.get(8)?,
        status: row.get(9)?,
        queued_time: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
        started_time: row.get::<_, Option<f64>>(11)?.unwrap_or(0.0),
        finished_time: row.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
        elapsed_s: row.get::<_, Option<f64>>(13)?.unwrap_or(0.0),
        result: row.get(14)?,
        error: row.get(15)?,
        updated_time: row.get::<_, Option<f64>>(16)?.unwrap_or(0.0),
        metadata: row
            .get::<_, Option<String>>(17)?
            .and_then(|value| SqliteStorage::json_from_str(&value)),
    })
}
