use super::PostgresStorage;
use crate::storage::{SessionRunRecord, StorageBackend};
use anyhow::Result;

pub(super) trait PostgresSessionRunStorage {
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

impl PostgresSessionRunStorage for PostgresStorage {
    fn upsert_session_run_impl(&self, record: &SessionRunRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO session_runs (run_id, session_id, parent_session_id, user_id, dispatch_id, run_kind, requested_by, agent_id, model_name, status, queued_time, \
             started_time, finished_time, elapsed_s, result, error, updated_time, metadata) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18) \
             ON CONFLICT(run_id) DO UPDATE SET session_id = EXCLUDED.session_id, parent_session_id = EXCLUDED.parent_session_id, \
             user_id = EXCLUDED.user_id, dispatch_id = EXCLUDED.dispatch_id, run_kind = EXCLUDED.run_kind, requested_by = EXCLUDED.requested_by, \
             agent_id = EXCLUDED.agent_id, model_name = EXCLUDED.model_name, status = EXCLUDED.status, \
             queued_time = EXCLUDED.queued_time, started_time = EXCLUDED.started_time, finished_time = EXCLUDED.finished_time, \
             elapsed_s = EXCLUDED.elapsed_s, result = EXCLUDED.result, error = EXCLUDED.error, updated_time = EXCLUDED.updated_time, \
             metadata = EXCLUDED.metadata",
            &[
                &record.run_id,
                &record.session_id,
                &record.parent_session_id,
                &record.user_id,
                &record.dispatch_id,
                &record.run_kind,
                &record.requested_by,
                &record.agent_id,
                &record.model_name,
                &record.status,
                &record.queued_time,
                &record.started_time,
                &record.finished_time,
                &record.elapsed_s,
                &record.result,
                &record.error,
                &record.updated_time,
                &metadata,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            &format!(
                "SELECT {} FROM session_runs WHERE run_id = $1",
                session_run_select_fields()
            ),
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_session_run_row(&row)))
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            &format!(
                "SELECT {} FROM session_runs WHERE user_id = $1 AND session_id = $2 \
                 ORDER BY updated_time DESC, queued_time DESC LIMIT $3",
                session_run_select_fields()
            ),
            &[&cleaned_user, &cleaned_session, &limit],
        )?;
        Ok(rows.iter().map(map_session_run_row).collect::<Vec<_>>())
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            &format!(
                "SELECT {} FROM session_runs WHERE user_id = $1 AND parent_session_id = $2 \
                 ORDER BY updated_time DESC, queued_time DESC LIMIT $3",
                session_run_select_fields()
            ),
            &[&cleaned_user, &cleaned_parent, &limit],
        )?;
        Ok(rows.iter().map(map_session_run_row).collect::<Vec<_>>())
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
        let mut conn = self.conn()?;
        let rows = conn.query(
            &format!(
                "SELECT {} FROM session_runs WHERE user_id = $1 AND dispatch_id = $2 \
                 ORDER BY updated_time DESC, queued_time DESC LIMIT $3",
                session_run_select_fields()
            ),
            &[&cleaned_user, &cleaned_dispatch, &limit],
        )?;
        Ok(rows.iter().map(map_session_run_row).collect::<Vec<_>>())
    }
}

fn session_run_select_fields() -> &'static str {
    "run_id, session_id, parent_session_id, user_id, dispatch_id, run_kind, requested_by, agent_id, model_name, status, queued_time, started_time, finished_time, elapsed_s, result, error, updated_time, metadata"
}

fn map_session_run_row(row: &tokio_postgres::Row) -> SessionRunRecord {
    SessionRunRecord {
        run_id: row.get(0),
        session_id: row.get(1),
        parent_session_id: row.get(2),
        user_id: row.get(3),
        dispatch_id: row.get(4),
        run_kind: row.get(5),
        requested_by: row.get(6),
        agent_id: row.get(7),
        model_name: row.get(8),
        status: row.get(9),
        queued_time: row.get::<_, Option<f64>>(10).unwrap_or(0.0),
        started_time: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
        finished_time: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
        elapsed_s: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
        result: row.get(14),
        error: row.get(15),
        updated_time: row.get::<_, Option<f64>>(16).unwrap_or(0.0),
        metadata: row
            .get::<_, Option<String>>(17)
            .and_then(|value| PostgresStorage::json_from_str(&value)),
    }
}
