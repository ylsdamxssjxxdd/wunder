use super::SqliteStorage;
use crate::storage::{ChatSessionRecord, StorageLifecycle};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};

pub(super) trait SqliteChatSessionStorage {
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

impl SqliteChatSessionStorage for SqliteStorage {
    fn upsert_chat_session_impl(&self, record: &ChatSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
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
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(session_id) DO UPDATE SET user_id = excluded.user_id, title = excluded.title, \
             status = excluded.status, created_at = excluded.created_at, updated_at = excluded.updated_at, \
             last_message_at = excluded.last_message_at, agent_id = excluded.agent_id, \
             tool_overrides = excluded.tool_overrides, parent_session_id = excluded.parent_session_id, \
             parent_message_id = excluded.parent_message_id, spawn_label = excluded.spawn_label, \
             spawned_by = excluded.spawned_by",
            params![
                record.session_id,
                record.user_id,
                record.title,
                status,
                record.created_at,
                record.updated_at,
                record.last_message_at,
                record.agent_id,
                tool_overrides,
                record.parent_session_id,
                record.parent_message_id,
                record.spawn_label,
                record.spawned_by
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                &format!(
                    "{} WHERE user_id = ? AND session_id = ?",
                    chat_session_select_sql()
                ),
                params![cleaned_user, cleaned_session],
                map_chat_session_row,
            )
            .optional()?;
        Ok(row)
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
        let conn = self.open()?;
        let agent_id = agent_id.map(|value| value.trim());
        let (agent_clause, agent_params) = match agent_id {
            None => ("".to_string(), Vec::new()),
            Some("") => (
                " AND (agent_id IS NULL OR agent_id = '')".to_string(),
                Vec::new(),
            ),
            Some(value) => (
                " AND agent_id = ?".to_string(),
                vec![SqlValue::from(value.to_string())],
            ),
        };
        let (parent_clause, parent_params) = match parent_session_id {
            None => ("".to_string(), Vec::new()),
            Some(value) if value.trim().is_empty() => (
                " AND (parent_session_id IS NULL OR parent_session_id = '')".to_string(),
                Vec::new(),
            ),
            Some(value) => (
                " AND parent_session_id = ?".to_string(),
                vec![SqlValue::from(value.trim().to_string())],
            ),
        };
        let normalized_status = status
            .map(str::trim)
            .map(str::to_lowercase)
            .unwrap_or_default();
        let (status_clause, status_params) =
            if normalized_status.is_empty() || normalized_status == "all" {
                ("".to_string(), Vec::new())
            } else if normalized_status == "archived" {
                (
                    " AND status = ?".to_string(),
                    vec![SqlValue::from("archived".to_string())],
                )
            } else {
                (
                    " AND (status IS NULL OR status = '' OR status = ?)".to_string(),
                    vec![SqlValue::from("active".to_string())],
                )
            };
        let total_sql = format!(
            "SELECT COUNT(*) FROM chat_sessions WHERE user_id = ?{agent_clause}{parent_clause}{status_clause}"
        );
        let mut total_params =
            Vec::with_capacity(1 + agent_params.len() + parent_params.len() + status_params.len());
        total_params.push(SqlValue::from(cleaned_user.to_string()));
        total_params.extend(agent_params.iter().cloned());
        total_params.extend(parent_params.iter().cloned());
        total_params.extend(status_params.iter().cloned());
        let total: i64 =
            conn.query_row(&total_sql, params_from_iter(total_params.iter()), |row| {
                row.get(0)
            })?;
        let mut sql = format!(
            "{} WHERE user_id = ?{agent_clause}{parent_clause}{status_clause} ORDER BY updated_at DESC",
            chat_session_select_sql()
        );
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        params_list.extend(agent_params);
        params_list.extend(parent_params);
        params_list.extend(status_params);
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), map_chat_session_row)?
            .collect::<std::result::Result<Vec<ChatSessionRecord>, _>>()?;
        Ok((rows, total))
    }

    fn list_chat_session_agent_ids_impl(&self, user_id: &str) -> Result<Vec<String>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT agent_id FROM chat_sessions \
                 WHERE user_id = ? AND (status IS NULL OR status = '' OR status = 'active')",
        )?;
        let rows = stmt.query_map([cleaned_user], |row| row.get::<_, Option<String>>(0))?;
        let mut agent_ids = Vec::new();
        for row in rows {
            let agent_id = row?.unwrap_or_default();
            agent_ids.push(agent_id);
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
        let conn = self.open()?;
        conn.execute(
            "UPDATE chat_sessions SET title = ?, updated_at = ? WHERE user_id = ? AND session_id = ?",
            params![title, updated_at, cleaned_user, cleaned_session],
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
        let conn = self.open()?;
        conn.execute(
            "UPDATE chat_sessions SET updated_at = ?, last_message_at = ? WHERE user_id = ? AND session_id = ?",
            params![updated_at, last_message_at, cleaned_user, cleaned_session],
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
        let conn = self.open()?;
        let _ = conn.execute(
            "DELETE FROM session_goals WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        );
        let affected = conn.execute(
            "DELETE FROM chat_sessions WHERE user_id = ? AND session_id = ?",
            params![cleaned_user, cleaned_session],
        )?;
        Ok(affected as i64)
    }
}

fn chat_session_select_sql() -> &'static str {
    "SELECT session_id, user_id, title, status, created_at, updated_at, last_message_at, agent_id, tool_overrides, \
     parent_session_id, parent_message_id, spawn_label, spawned_by FROM chat_sessions"
}

fn map_chat_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChatSessionRecord> {
    let tool_overrides: Option<String> = row.get(8)?;
    let status = row
        .get::<_, Option<String>>(3)?
        .unwrap_or_else(|| "active".to_string());
    Ok(ChatSessionRecord {
        session_id: row.get(0)?,
        user_id: row.get(1)?,
        title: row.get(2)?,
        status: if status.trim().is_empty() {
            "active".to_string()
        } else {
            status
        },
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        last_message_at: row.get(6)?,
        agent_id: row.get(7)?,
        tool_overrides: SqliteStorage::parse_string_list(tool_overrides),
        parent_session_id: row.get(9)?,
        parent_message_id: row.get(10)?,
        spawn_label: row.get(11)?,
        spawned_by: row.get(12)?,
    })
}
