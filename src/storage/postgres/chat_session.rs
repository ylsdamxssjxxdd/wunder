use super::PostgresStorage;
use crate::storage::{ChatSessionRecord, StorageBackend};
use anyhow::Result;
use tokio_postgres::types::ToSql;

pub(super) trait PostgresChatSessionStorage {
    fn upsert_chat_session_impl(&self, record: &ChatSessionRecord) -> Result<()>;
    fn get_chat_session_impl(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>>;
    fn list_chat_sessions_impl(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn list_chat_sessions_by_status_impl(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)>;
    fn list_chat_session_agent_ids_impl(&self, user_id: &str) -> Result<Vec<String>>;
    fn update_chat_session_title_impl(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()>;
    fn touch_chat_session_impl(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()>;
    fn delete_chat_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64>;
}

impl PostgresChatSessionStorage for PostgresStorage {
    fn upsert_chat_session_impl(&self, record: &ChatSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        let status = {
            let cleaned = record.status.trim().to_lowercase();
            if cleaned.is_empty() {
                "active".to_string()
            } else {
                cleaned
            }
        };
        conn.execute(
            "INSERT INTO chat_sessions (session_id, user_id, title, status, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
             parent_session_id, parent_message_id, spawn_label, spawned_by) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = EXCLUDED.user_id, title = EXCLUDED.title, status = EXCLUDED.status, \
             created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at, last_message_at = EXCLUDED.last_message_at, \
             agent_id = EXCLUDED.agent_id, tool_overrides = EXCLUDED.tool_overrides, parent_session_id = EXCLUDED.parent_session_id, \
             parent_message_id = EXCLUDED.parent_message_id, spawn_label = EXCLUDED.spawn_label, spawned_by = EXCLUDED.spawned_by",
            &[
                &record.session_id,
                &record.user_id,
                &record.title,
                &status,
                &record.created_at,
                &record.updated_at,
                &record.last_message_at,
                &record.agent_id,
                &tool_overrides,
                &record.parent_session_id,
                &record.parent_message_id,
                &record.spawn_label,
                &record.spawned_by,
            ],
        )?;
        Ok(())
    }

    fn get_chat_session_impl(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<ChatSessionRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            &format!(
                "{} WHERE user_id = $1 AND session_id = $2",
                chat_session_select_sql()
            ),
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(row.map(|row| map_chat_session_row(&row)))
    }

    fn list_chat_sessions_impl(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.list_chat_sessions_by_status_impl(
            user_id,
            agent_id,
            parent_session_id,
            Some("active"),
            offset,
            limit,
        )
    }

    fn list_chat_sessions_by_status_impl(
        &self,
        user_id: &str,
        agent_id: Option<&str>,
        parent_session_id: Option<&str>,
        status: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChatSessionRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let mut conn = self.conn()?;
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        params.push(Box::new(cleaned_user.to_string()));
        conditions.push(format!("user_id = ${}", params.len()));

        let agent_id = agent_id.map(|value| value.trim());
        match agent_id {
            None => {}
            Some("") => {
                conditions.push("(agent_id IS NULL OR agent_id = '')".to_string());
            }
            Some(value) => {
                params.push(Box::new(value.to_string()));
                conditions.push(format!("agent_id = ${}", params.len()));
            }
        }

        match parent_session_id {
            None => {}
            Some(value) if value.trim().is_empty() => {
                conditions
                    .push("(parent_session_id IS NULL OR parent_session_id = '')".to_string());
            }
            Some(value) => {
                params.push(Box::new(value.trim().to_string()));
                conditions.push(format!("parent_session_id = ${}", params.len()));
            }
        }

        let normalized_status = status
            .map(str::trim)
            .map(str::to_lowercase)
            .unwrap_or_default();
        if !(normalized_status.is_empty() || normalized_status == "all") {
            if normalized_status == "archived" {
                params.push(Box::new("archived".to_string()));
                conditions.push(format!("status = ${}", params.len()));
            } else {
                params.push(Box::new("active".to_string()));
                conditions.push(format!(
                    "(status IS NULL OR status = '' OR status = ${})",
                    params.len()
                ));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let count_sql = format!("SELECT COUNT(*) FROM chat_sessions{where_clause}");
        let params_ref: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(|value| value.as_ref()).collect();
        let total: i64 = conn.query_one(&count_sql, &params_ref)?.get(0);

        let mut sql = format!(
            "{}{where_clause} ORDER BY updated_at DESC",
            chat_session_select_sql()
        );
        let mut list_params: Vec<Box<dyn ToSql + Sync>> = params;
        if limit > 0 {
            list_params.push(Box::new(limit));
            list_params.push(Box::new(offset.max(0)));
            sql.push_str(&format!(
                " LIMIT ${} OFFSET ${}",
                list_params.len() - 1,
                list_params.len()
            ));
        }
        let list_ref: Vec<&(dyn ToSql + Sync)> =
            list_params.iter().map(|value| value.as_ref()).collect();
        let rows = conn.query(&sql, &list_ref)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_chat_session_row(&row));
        }
        Ok((output, total))
    }

    fn list_chat_session_agent_ids_impl(&self, user_id: &str) -> Result<Vec<String>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT DISTINCT agent_id FROM chat_sessions \
             WHERE user_id = $1 AND (status IS NULL OR status = '' OR status = 'active')",
            &[&cleaned_user],
        )?;
        let mut agent_ids = Vec::new();
        for row in rows {
            let agent_id: Option<String> = row.get(0);
            agent_ids.push(agent_id.unwrap_or_default());
        }
        Ok(agent_ids)
    }

    fn update_chat_session_title_impl(
        &self,
        user_id: &str,
        session_id: &str,
        title: &str,
        updated_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE chat_sessions SET title = $1, updated_at = $2 WHERE user_id = $3 AND session_id = $4",
            &[&title, &updated_at, &cleaned_user, &cleaned_session],
        )?;
        Ok(())
    }

    fn touch_chat_session_impl(
        &self,
        user_id: &str,
        session_id: &str,
        updated_at: f64,
        last_message_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE chat_sessions SET updated_at = $1, last_message_at = $2 WHERE user_id = $3 AND session_id = $4",
            &[&updated_at, &last_message_at, &cleaned_user, &cleaned_session],
        )?;
        Ok(())
    }

    fn delete_chat_session_impl(&self, user_id: &str, session_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_session = session_id.trim();
        if cleaned_user.is_empty() || cleaned_session.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let _ = conn.execute(
            "DELETE FROM session_goals WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        );
        let affected = conn.execute(
            "DELETE FROM chat_sessions WHERE user_id = $1 AND session_id = $2",
            &[&cleaned_user, &cleaned_session],
        )?;
        Ok(affected as i64)
    }
}

fn chat_session_select_sql() -> &'static str {
    "SELECT session_id, user_id, title, status, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
     parent_session_id, parent_message_id, spawn_label, spawned_by FROM chat_sessions"
}

fn map_chat_session_row(row: &tokio_postgres::Row) -> ChatSessionRecord {
    ChatSessionRecord {
        session_id: row.get(0),
        user_id: row.get(1),
        title: row.get(2),
        status: {
            let status: Option<String> = row.get(3);
            let normalized = status.unwrap_or_else(|| "active".to_string());
            if normalized.trim().is_empty() {
                "active".to_string()
            } else {
                normalized
            }
        },
        created_at: row.get(4),
        updated_at: row.get(5),
        last_message_at: row.get(6),
        agent_id: row.get(7),
        tool_overrides: PostgresStorage::parse_string_list(row.get(8)),
        parent_session_id: row.get(9),
        parent_message_id: row.get(10),
        spawn_label: row.get(11),
        spawned_by: row.get(12),
    }
}
