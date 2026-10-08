use super::PostgresStorage;
use crate::storage::{SessionGoalRecord, StorageLifecycle};
use anyhow::Result;

pub(super) trait PostgresSessionGoalStorage {
    fn upsert_session_goal_impl(&self, record: &SessionGoalRecord) -> Result<()>;
    fn update_session_goal_impl(
        &self,
        record: &SessionGoalRecord,
        expected_revision: i64,
    ) -> Result<bool>;
    fn get_session_goal_impl(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionGoalRecord>>;
    fn list_session_goals_impl(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<SessionGoalRecord>>;
    fn delete_session_goal_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
}

impl PostgresSessionGoalStorage for PostgresStorage {
    fn upsert_session_goal_impl(&self, record: &SessionGoalRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        let cleaned_session = record.session_id.trim();
        let cleaned_goal = record.goal_id.trim();
        let cleaned_objective = record.objective.trim();
        let cleaned_phase = record.phase.trim();
        if cleaned_user.is_empty()
            || cleaned_session.is_empty()
            || cleaned_goal.is_empty()
            || cleaned_objective.is_empty()
            || cleaned_phase.is_empty()
        {
            return Ok(());
        }
        let revision = record.revision.max(1);
        let max_goal_rounds = record.max_goal_rounds.max(1);
        let rounds_started = record.rounds_started.max(0);
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO session_goals (
                session_id, user_id, goal_id, revision, objective, phase,
                blocked_code, blocked_message, max_goal_rounds, rounds_started,
                created_at, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT(session_id) DO UPDATE SET
                user_id = EXCLUDED.user_id,
                goal_id = EXCLUDED.goal_id,
                revision = EXCLUDED.revision,
                objective = EXCLUDED.objective,
                phase = EXCLUDED.phase,
                blocked_code = EXCLUDED.blocked_code,
                blocked_message = EXCLUDED.blocked_message,
                max_goal_rounds = EXCLUDED.max_goal_rounds,
                rounds_started = EXCLUDED.rounds_started,
                created_at = EXCLUDED.created_at,
                updated_at = EXCLUDED.updated_at",
            &[
                &cleaned_session,
                &cleaned_user,
                &cleaned_goal,
                &revision,
                &cleaned_objective,
                &cleaned_phase,
                &record.blocked_code,
                &record.blocked_message,
                &max_goal_rounds,
                &rounds_started,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn update_session_goal_impl(
        &self,
        record: &SessionGoalRecord,
        expected_revision: i64,
    ) -> Result<bool> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        let cleaned_session = record.session_id.trim();
        let cleaned_goal = record.goal_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() || cleaned_goal.is_empty() {
            return Ok(false);
        }
        let revision = record.revision.max(1);
        let max_goal_rounds = record.max_goal_rounds.max(1);
        let rounds_started = record.rounds_started.max(0);
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "UPDATE session_goals SET
                revision = $1, objective = $2, phase = $3, blocked_code = $4, blocked_message = $5,
                max_goal_rounds = $6, rounds_started = $7, updated_at = $8
             WHERE user_id = $9 AND session_id = $10 AND goal_id = $11 AND revision = $12",
            &[
                &revision,
                &record.objective.trim(),
                &record.phase.trim(),
                &record.blocked_code,
                &record.blocked_message,
                &max_goal_rounds,
                &rounds_started,
                &record.updated_at,
                &cleaned_user,
                &cleaned_session,
                &cleaned_goal,
                &expected_revision,
            ],
        )?;
        Ok(affected > 0)
    }

    fn get_session_goal_impl(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionGoalRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            session_goal_select_sql("WHERE user_id = $1 AND session_id = $2").as_str(),
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(row.map(map_session_goal_row))
    }

    fn list_session_goals_impl(
        &self,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<SessionGoalRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let cleaned_sessions = session_ids
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if cleaned_sessions.is_empty() {
            return Ok(Vec::new());
        }
        let cleaned_user = cleaned_user.to_string();
        let mut conn = self.conn()?;
        let rows = conn.query(
            session_goal_select_sql("WHERE user_id = $1 AND session_id = ANY($2::TEXT[])").as_str(),
            &[&cleaned_user, &cleaned_sessions],
        )?;
        Ok(rows.into_iter().map(map_session_goal_row).collect())
    }

    fn delete_session_goal_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM session_goals WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }
}

fn session_goal_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT goal_id, session_id, user_id, revision, objective, phase,
         blocked_code, blocked_message, max_goal_rounds, rounds_started, created_at, updated_at
         FROM session_goals {where_clause}"
    )
}

fn map_session_goal_row(row: tokio_postgres::Row) -> SessionGoalRecord {
    SessionGoalRecord {
        goal_id: row.get(0),
        session_id: row.get(1),
        user_id: row.get(2),
        revision: row.get::<_, Option<i64>>(3).unwrap_or(1).max(1),
        objective: row.get(4),
        phase: row.get(5),
        blocked_code: row.get(6),
        blocked_message: row.get(7),
        max_goal_rounds: row.get::<_, Option<i64>>(8).unwrap_or(256).max(1),
        rounds_started: row.get::<_, Option<i64>>(9).unwrap_or(0).max(0),
        created_at: row.get(10),
        updated_at: row.get(11),
    }
}
