use super::SqliteStorage;
use crate::storage::{SessionGoalRecord, StorageBackend};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};

pub(super) trait SqliteSessionGoalStorage {
    fn upsert_session_goal_impl(&self, record: &SessionGoalRecord) -> Result<()>;
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
    fn account_session_goal_usage_impl(
        &self,
        user_id: &str,
        session_id: &str,
        tokens_delta: i64,
        time_delta_seconds: i64,
        updated_at: f64,
    ) -> Result<Option<SessionGoalRecord>>;
}

impl SqliteSessionGoalStorage for SqliteStorage {
    fn upsert_session_goal_impl(&self, record: &SessionGoalRecord) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = record.user_id.trim();
        let cleaned_session = record.session_id.trim();
        let cleaned_goal = record.goal_id.trim();
        let cleaned_objective = record.objective.trim();
        let cleaned_status = record.status.trim();
        if cleaned_user.is_empty()
            || cleaned_session.is_empty()
            || cleaned_goal.is_empty()
            || cleaned_objective.is_empty()
            || cleaned_status.is_empty()
        {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO session_goals (
                session_id, user_id, goal_id, objective, status, token_budget, tokens_used,
                time_used_seconds, created_at, updated_at, completed_at, last_continued_at, source
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
                user_id = excluded.user_id,
                goal_id = excluded.goal_id,
                objective = excluded.objective,
                status = excluded.status,
                token_budget = excluded.token_budget,
                tokens_used = excluded.tokens_used,
                time_used_seconds = excluded.time_used_seconds,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                last_continued_at = excluded.last_continued_at,
                source = excluded.source",
            params![
                cleaned_session,
                cleaned_user,
                cleaned_goal,
                cleaned_objective,
                cleaned_status,
                record.token_budget,
                record.tokens_used.max(0),
                record.time_used_seconds.max(0),
                record.created_at,
                record.updated_at,
                record.completed_at,
                record.last_continued_at,
                record.source.trim()
            ],
        )?;
        Ok(())
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

    fn account_session_goal_usage_impl(
        &self,
        user_id: &str,
        session_id: &str,
        tokens_delta: i64,
        time_delta_seconds: i64,
        updated_at: f64,
    ) -> Result<Option<SessionGoalRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        conn.execute(
            "UPDATE session_goals
             SET tokens_used = MAX(tokens_used + ?, 0),
                 time_used_seconds = MAX(time_used_seconds + ?, 0),
                 updated_at = ?
             WHERE user_id = ? AND session_id = ?",
            params![
                tokens_delta,
                time_delta_seconds,
                updated_at,
                cleaned_user,
                cleaned_session
            ],
        )?;
        let row = conn
            .query_row(
                session_goal_select_sql("WHERE user_id = ? AND session_id = ?").as_str(),
                params![cleaned_user, cleaned_session],
                map_session_goal_row,
            )
            .optional()?;
        Ok(row)
    }
}

fn session_goal_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT goal_id, session_id, user_id, objective, status, token_budget, tokens_used,
         time_used_seconds, created_at, updated_at, completed_at, last_continued_at, source
         FROM session_goals {where_clause}"
    )
}

fn map_session_goal_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionGoalRecord> {
    Ok(SessionGoalRecord {
        goal_id: row.get(0)?,
        session_id: row.get(1)?,
        user_id: row.get(2)?,
        objective: row.get(3)?,
        status: row.get(4)?,
        token_budget: row.get(5)?,
        tokens_used: row.get::<_, Option<i64>>(6)?.unwrap_or(0).max(0),
        time_used_seconds: row.get::<_, Option<i64>>(7)?.unwrap_or(0).max(0),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        completed_at: row.get(10)?,
        last_continued_at: row.get(11)?,
        source: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
    })
}
