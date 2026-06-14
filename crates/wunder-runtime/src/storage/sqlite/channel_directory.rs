use super::SqliteStorage;
use crate::storage::{
    ChannelAccountRecord, ChannelBindingRecord, ChannelUserBindingRecord,
    ListChannelUserBindingsQuery, StorageBackend,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde_json::Value;

pub(super) trait SqliteChannelDirectoryStorage {
    fn upsert_channel_account_impl(&self, record: &ChannelAccountRecord) -> Result<()>;
    fn get_channel_account_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>>;
    fn list_channel_accounts_impl(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>>;
    fn delete_channel_account_impl(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn upsert_channel_binding_impl(&self, record: &ChannelBindingRecord) -> Result<()>;
    fn list_channel_bindings_impl(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelBindingRecord>>;
    fn delete_channel_binding_impl(&self, binding_id: &str) -> Result<i64>;
    fn upsert_channel_user_binding_impl(&self, record: &ChannelUserBindingRecord) -> Result<()>;
    fn get_channel_user_binding_impl(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>>;
    fn list_channel_user_bindings_impl(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)>;
    fn delete_channel_user_binding_impl(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64>;
}

impl SqliteChannelDirectoryStorage for SqliteStorage {
    fn upsert_channel_account_impl(&self, record: &ChannelAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let config = Self::json_to_string(&record.config);
        conn.execute(
            "INSERT INTO channel_accounts (channel, account_id, config, status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(channel, account_id) DO UPDATE SET config = excluded.config, status = excluded.status, updated_at = excluded.updated_at",
            params![
                record.channel,
                record.account_id,
                config,
                record.status,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_channel_account_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<ChannelAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts WHERE channel = ? AND account_id = ?",
                params![cleaned_channel, cleaned_account],
                map_channel_account_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_channel_accounts_impl(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(status) = status
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        let mut query =
            "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts"
                .to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(
            params_from_iter(params_list.iter()),
            map_channel_account_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_channel_account_impl(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_accounts WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_binding_impl(&self, record: &ChannelBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        let enabled = if record.enabled { 1 } else { 0 };
        conn.execute(
            "INSERT INTO channel_bindings (binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(binding_id) DO UPDATE SET channel = excluded.channel, account_id = excluded.account_id, peer_kind = excluded.peer_kind, peer_id = excluded.peer_id, \
             agent_id = excluded.agent_id, tool_overrides = excluded.tool_overrides, priority = excluded.priority, enabled = excluded.enabled, updated_at = excluded.updated_at",
            params![
                record.binding_id,
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.agent_id,
                tool_overrides,
                record.priority,
                enabled,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn list_channel_bindings_impl(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelBindingRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = "SELECT binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at FROM channel_bindings".to_string();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            query.push_str(" WHERE channel = ?");
            params_list.push(SqlValue::from(channel.to_string()));
        }
        query.push_str(" ORDER BY priority DESC, updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(
            params_from_iter(params_list.iter()),
            map_channel_binding_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_channel_binding_impl(&self, binding_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = binding_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_bindings WHERE binding_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_user_binding_impl(&self, record: &ChannelUserBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO channel_user_bindings (channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id) DO UPDATE SET user_id = excluded.user_id, updated_at = excluded.updated_at",
            params![
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.user_id,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_channel_user_binding_impl(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<Option<ChannelUserBindingRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at \
                 FROM channel_user_bindings WHERE channel = ? AND account_id = ? AND peer_kind = ? AND peer_id = ?",
                params![channel, account_id, peer_kind, peer_id],
                map_channel_user_binding_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_channel_user_bindings_impl(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        push_optional_filter(&mut filters, &mut params_list, "channel", query.channel);
        push_optional_filter(
            &mut filters,
            &mut params_list,
            "account_id",
            query.account_id,
        );
        push_optional_filter(&mut filters, &mut params_list, "peer_kind", query.peer_kind);
        push_optional_filter(&mut filters, &mut params_list, "peer_id", query.peer_id);
        push_optional_filter(&mut filters, &mut params_list, "user_id", query.user_id);

        let mut sql =
            "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at FROM channel_user_bindings"
                .to_string();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let offset_value = query.offset.max(0);
        let limit_value = if query.limit <= 0 {
            100
        } else {
            query.limit.min(500)
        };
        params_list.push(SqlValue::from(limit_value));
        params_list.push(SqlValue::from(offset_value));
        sql.push_str(" LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params_from_iter(params_list.iter()),
            map_channel_user_binding_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        let mut count_sql = "SELECT COUNT(*) FROM channel_user_bindings".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_sql, count_params, |row| row.get(0))?;
        Ok((output, total))
    }

    fn delete_channel_user_binding_impl(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        let cleaned_kind = peer_kind.trim();
        let cleaned_peer = peer_id.trim();
        if cleaned_channel.is_empty()
            || cleaned_account.is_empty()
            || cleaned_kind.is_empty()
            || cleaned_peer.is_empty()
        {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_user_bindings WHERE channel = ? AND account_id = ? AND peer_kind = ? AND peer_id = ?",
            params![cleaned_channel, cleaned_account, cleaned_kind, cleaned_peer],
        )?;
        Ok(affected as i64)
    }
}

fn push_optional_filter(
    filters: &mut Vec<String>,
    params_list: &mut Vec<SqlValue>,
    column: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        filters.push(format!("{column} = ?"));
        params_list.push(SqlValue::from(value.to_string()));
    }
}

fn map_channel_account_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChannelAccountRecord> {
    let config_text: String = row.get(2)?;
    Ok(ChannelAccountRecord {
        channel: row.get(0)?,
        account_id: row.get(1)?,
        config: SqliteStorage::json_from_str(&config_text).unwrap_or(Value::Null),
        status: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_channel_binding_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChannelBindingRecord> {
    let tool_overrides: Option<String> = row.get(6)?;
    Ok(ChannelBindingRecord {
        binding_id: row.get(0)?,
        channel: row.get(1)?,
        account_id: row.get(2)?,
        peer_kind: row.get(3)?,
        peer_id: row.get(4)?,
        agent_id: row.get(5)?,
        tool_overrides: SqliteStorage::parse_string_list(tool_overrides),
        priority: row.get(7)?,
        enabled: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn map_channel_user_binding_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ChannelUserBindingRecord> {
    Ok(ChannelUserBindingRecord {
        channel: row.get(0)?,
        account_id: row.get(1)?,
        peer_kind: row.get(2)?,
        peer_id: row.get(3)?,
        user_id: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}
