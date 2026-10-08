use super::SqliteStorage;
use crate::storage::{SessionGoalRecord, StorageLifecycle};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};

pub(super) trait SqliteSessionGoalStorage {
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

impl SqliteSessionGoalStorage for SqliteStorage {
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
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO session_goals (
                session_id, user_id, goal_id, revision, objective, phase,
                blocked_code, blocked_message, max_goal_rounds, rounds_started,
                created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
                user_id = excluded.user_id,
                goal_id = excluded.goal_id,
                revision = excluded.revision,
                objective = excluded.objective,
                phase = excluded.phase,
                blocked_code = excluded.blocked_code,
                blocked_message = excluded.blocked_message,
                max_goal_rounds = excluded.max_goal_rounds,
                rounds_started = excluded.rounds_started,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at",
            params![
                cleaned_session,
                cleaned_user,
                cleaned_goal,
                record.revision.max(1),
                cleaned_objective,
                cleaned_phase,
                record.blocked_code,
                record.blocked_message,
                record.max_goal_rounds.max(1),
                record.rounds_started.max(0),
                record.created_at,
                record.updated_at
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
        let conn = self.open()?;
        let affected = conn.execute(
            "UPDATE session_goals SET
                revision = ?, objective = ?, phase = ?, blocked_code = ?, blocked_message = ?,
                max_goal_rounds = ?, rounds_started = ?, updated_at = ?
             WHERE user_id = ? AND session_id = ? AND goal_id = ? AND revision = ?",
            params![
                record.revision.max(1),
                record.objective.trim(),
                record.phase.trim(),
                record.blocked_code,
                record.blocked_message,
                record.max_goal_rounds.max(1),
                record.rounds_started.max(0),
                record.updated_at,
                cleaned_user,
                cleaned_session,
                cleaned_goal,
                expected_revision
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                session_goal_select_sql("WHERE user_id = ? AND session_id = ?").as_str(),
                params![cleaned_user, cleaned_session],
                map_session_goal_row,
            )
            .optional()?;
        Ok(row)
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
            .collect::<Vec<_>>();
        if cleaned_sessions.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let placeholders = vec!["?"; cleaned_sessions.len()].join(", ");
        let sql = session_goal_select_sql(&format!(
            "WHERE user_id = ? AND session_id IN ({placeholders})"
        ));
        let mut params_list = Vec::with_capacity(1 + cleaned_sessions.len());
        params_list.push(SqlValue::from(cleaned_user.to_string()));
        params_list.extend(
            cleaned_sessions
                .iter()
                .map(|value| SqlValue::from((*value).to_string())),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), map_session_goal_row)?;
        let goals = rows.collect::<std::result::Result<Vec<SessionGoalRecord>, _>>()?;
        Ok(goals)
    }

    fn delete_session_goal_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM session_goals WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
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

fn map_session_goal_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionGoalRecord> {
    Ok(SessionGoalRecord {
        goal_id: row.get(0)?,
        session_id: row.get(1)?,
        user_id: row.get(2)?,
        revision: row.get::<_, Option<i64>>(3)?.unwrap_or(1).max(1),
        objective: row.get(4)?,
        phase: row.get(5)?,
        blocked_code: row.get(6)?,
        blocked_message: row.get(7)?,
        max_goal_rounds: row.get::<_, Option<i64>>(8)?.unwrap_or(256).max(1),
        rounds_started: row.get::<_, Option<i64>>(9)?.unwrap_or(0).max(0),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}
