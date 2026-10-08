use super::PostgresStorage;
use crate::storage::{
    ChannelAccountRecord, ChannelBindingRecord, ChannelUserBindingRecord,
    ListChannelUserBindingsQuery, StorageLifecycle,
};
use anyhow::Result;
use serde_json::Value;
use tokio_postgres::types::ToSql;

pub(super) trait PostgresChannelDirectoryStorage {
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

impl PostgresChannelDirectoryStorage for PostgresStorage {
    fn upsert_channel_account_impl(&self, record: &ChannelAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let config = Self::json_to_string(&record.config);
        conn.execute(
            "INSERT INTO channel_accounts (channel, account_id, config, status, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT(channel, account_id) DO UPDATE SET config = EXCLUDED.config, status = EXCLUDED.status, updated_at = EXCLUDED.updated_at",
            &[
                &record.channel,
                &record.account_id,
                &config,
                &record.status,
                &record.created_at,
                &record.updated_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(row.map(|row| map_channel_account_row(&row)))
    }

    fn list_channel_accounts_impl(
        &self,
        channel: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<ChannelAccountRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        push_optional_filter(&mut filters, &mut params, "channel", channel);
        push_optional_filter(&mut filters, &mut params, "status", status);
        let mut query = "SELECT channel, account_id, config, status, created_at, updated_at FROM channel_accounts".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(Box::as_ref).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_channel_account_row(&row));
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
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_accounts WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_binding_impl(&self, record: &ChannelBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let tool_overrides = if record.tool_overrides.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.tool_overrides))
        };
        let enabled = if record.enabled { 1 } else { 0 };
        conn.execute(
            "INSERT INTO channel_bindings (binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
             ON CONFLICT(binding_id) DO UPDATE SET channel = EXCLUDED.channel, account_id = EXCLUDED.account_id, peer_kind = EXCLUDED.peer_kind, peer_id = EXCLUDED.peer_id, \
             agent_id = EXCLUDED.agent_id, tool_overrides = EXCLUDED.tool_overrides, priority = EXCLUDED.priority, enabled = EXCLUDED.enabled, updated_at = EXCLUDED.updated_at",
            &[
                &record.binding_id,
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.agent_id,
                &tool_overrides,
                &record.priority,
                &enabled,
                &record.created_at,
                &record.updated_at,
            ],
        )?;
        Ok(())
    }

    fn list_channel_bindings_impl(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelBindingRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = "SELECT binding_id, channel, account_id, peer_kind, peer_id, agent_id, tool_overrides, priority, enabled, created_at, updated_at FROM channel_bindings".to_string();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            query.push_str(&format!(" WHERE channel = ${}", params.len()));
        }
        query.push_str(" ORDER BY priority DESC, updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(Box::as_ref).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_channel_binding_row(&row));
        }
        Ok(output)
    }

    fn delete_channel_binding_impl(&self, binding_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = binding_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_bindings WHERE binding_id = $1",
            &[&cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_channel_user_binding_impl(&self, record: &ChannelUserBindingRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO channel_user_bindings (channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id) DO UPDATE SET user_id = EXCLUDED.user_id, updated_at = EXCLUDED.updated_at",
            &[
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.user_id,
                &record.created_at,
                &record.updated_at,
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
        let mut conn = self.conn()?;
        let row = conn
            .query_opt(
                "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at \
                 FROM channel_user_bindings WHERE channel = $1 AND account_id = $2 AND peer_kind = $3 AND peer_id = $4",
                &[&channel, &account_id, &peer_kind, &peer_id],
            )?
            .map(|row| map_channel_user_binding_row(&row));
        Ok(row)
    }

    fn list_channel_user_bindings_impl(
        &self,
        query: ListChannelUserBindingsQuery<'_>,
    ) -> Result<(Vec<ChannelUserBindingRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        push_optional_filter(&mut filters, &mut params, "channel", query.channel);
        push_optional_filter(&mut filters, &mut params, "account_id", query.account_id);
        push_optional_filter(&mut filters, &mut params, "peer_kind", query.peer_kind);
        push_optional_filter(&mut filters, &mut params, "peer_id", query.peer_id);
        push_optional_filter(&mut filters, &mut params, "user_id", query.user_id);

        let mut sql = "SELECT channel, account_id, peer_kind, peer_id, user_id, created_at, updated_at FROM channel_user_bindings".to_string();
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
        params.push(Box::new(offset_value));
        params.push(Box::new(limit_value));
        sql.push_str(&format!(
            " OFFSET ${} LIMIT ${}",
            params.len() - 1,
            params.len()
        ));
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(Box::as_ref).collect();
        let rows = conn.query(&sql, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_channel_user_binding_row(&row));
        }
        let mut count_sql = "SELECT COUNT(*) FROM channel_user_bindings".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params: Vec<&(dyn ToSql + Sync)> = params_refs[..params_refs.len() - 2].to_vec();
        let total_row = conn.query_one(&count_sql, &count_params)?;
        let total: i64 = total_row.get(0);
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
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_user_bindings WHERE channel = $1 AND account_id = $2 AND peer_kind = $3 AND peer_id = $4",
            &[&cleaned_channel, &cleaned_account, &cleaned_kind, &cleaned_peer],
        )?;
        Ok(affected as i64)
    }
}

fn push_optional_filter(
    filters: &mut Vec<String>,
    params: &mut Vec<Box<dyn ToSql + Sync>>,
    column: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        params.push(Box::new(value.to_string()));
        filters.push(format!("{column} = ${}", params.len()));
    }
}

fn map_channel_account_row(row: &tokio_postgres::Row) -> ChannelAccountRecord {
    ChannelAccountRecord {
        channel: row.get(0),
        account_id: row.get(1),
        config: PostgresStorage::json_from_str(row.get::<_, String>(2).as_str())
            .unwrap_or(Value::Null),
        status: row.get(3),
        created_at: row.get(4),
        updated_at: row.get(5),
    }
}

fn map_channel_binding_row(row: &tokio_postgres::Row) -> ChannelBindingRecord {
    let tool_overrides: Option<String> = row.get(6);
    ChannelBindingRecord {
        binding_id: row.get(0),
        channel: row.get(1),
        account_id: row.get(2),
        peer_kind: row.get(3),
        peer_id: row.get(4),
        agent_id: row.get(5),
        tool_overrides: PostgresStorage::parse_string_list(tool_overrides),
        priority: row.get::<_, i64>(7),
        enabled: row.get::<_, i32>(8) != 0,
        created_at: row.get(9),
        updated_at: row.get(10),
    }
}

fn map_channel_user_binding_row(row: &tokio_postgres::Row) -> ChannelUserBindingRecord {
    ChannelUserBindingRecord {
        channel: row.get(0),
        account_id: row.get(1),
        peer_kind: row.get(2),
        peer_id: row.get(3),
        user_id: row.get(4),
        created_at: row.get::<_, Option<f64>>(5).unwrap_or(0.0),
        updated_at: row.get::<_, Option<f64>>(6).unwrap_or(0.0),
    }
}
