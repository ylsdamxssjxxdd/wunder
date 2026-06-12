use super::PostgresStorage;
use crate::storage::{SessionGoalRecord, StorageBackend};
use anyhow::Result;

pub(super) trait PostgresSessionGoalStorage {
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

impl PostgresSessionGoalStorage for PostgresStorage {
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
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO session_goals (
                session_id, user_id, goal_id, objective, status, token_budget, tokens_used,
                time_used_seconds, created_at, updated_at, completed_at, last_continued_at, source
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
             ON CONFLICT(session_id) DO UPDATE SET
                user_id = EXCLUDED.user_id,
                goal_id = EXCLUDED.goal_id,
                objective = EXCLUDED.objective,
                status = EXCLUDED.status,
                token_budget = EXCLUDED.token_budget,
                tokens_used = EXCLUDED.tokens_used,
                time_used_seconds = EXCLUDED.time_used_seconds,
                created_at = EXCLUDED.created_at,
                updated_at = EXCLUDED.updated_at,
                completed_at = EXCLUDED.completed_at,
                last_continued_at = EXCLUDED.last_continued_at,
                source = EXCLUDED.source",
            &[
                &cleaned_session,
                &cleaned_user,
                &cleaned_goal,
                &cleaned_objective,
                &cleaned_status,
                &record.token_budget,
                &record.tokens_used.max(0),
                &record.time_used_seconds.max(0),
                &record.created_at,
                &record.updated_at,
                &record.completed_at,
                &record.last_continued_at,
                &record.source.trim(),
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
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE session_goals
             SET tokens_used = GREATEST(tokens_used + $1, 0),
                 time_used_seconds = GREATEST(time_used_seconds + $2, 0),
                 updated_at = $3
             WHERE user_id = $4 AND session_id = $5",
            &[
                &tokens_delta,
                &time_delta_seconds,
                &updated_at,
                &cleaned_user,
                &cleaned_session,
            ],
        )?;
        let row = conn.query_opt(
            session_goal_select_sql("WHERE user_id = $1 AND session_id = $2").as_str(),
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(row.map(map_session_goal_row))
    }
}

fn session_goal_select_sql(where_clause: &str) -> String {
    format!(
        "SELECT goal_id, session_id, user_id, objective, status, token_budget, tokens_used,
         time_used_seconds, created_at, updated_at, completed_at, last_continued_at, source
         FROM session_goals {where_clause}"
    )
}

fn map_session_goal_row(row: tokio_postgres::Row) -> SessionGoalRecord {
    SessionGoalRecord {
        goal_id: row.get(0),
        session_id: row.get(1),
        user_id: row.get(2),
        objective: row.get(3),
        status: row.get(4),
        token_budget: row.get(5),
        tokens_used: row.get::<_, Option<i64>>(6).unwrap_or(0).max(0),
        time_used_seconds: row.get::<_, Option<i64>>(7).unwrap_or(0).max(0),
        created_at: row.get(8),
        updated_at: row.get(9),
        completed_at: row.get(10),
        last_continued_at: row.get(11),
        source: row.get::<_, Option<String>>(12).unwrap_or_default(),
    }
}
