use super::SqliteStorage;
use crate::storage::{
    BridgeCenterAccountRecord, BridgeCenterRecord, BridgeDeliveryLogRecord,
    BridgeRouteAuditLogRecord, BridgeUserRouteRecord, ListBridgeCenterAccountsQuery,
    ListBridgeCentersQuery, ListBridgeDeliveryLogsQuery, ListBridgeRouteAuditLogsQuery,
    ListBridgeUserRoutesQuery, StorageBackend,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};
use serde_json::Value;

pub(super) trait SqliteBridgeStorage {
    fn upsert_bridge_center_impl(&self, record: &BridgeCenterRecord) -> Result<()>;
    fn get_bridge_center_impl(&self, center_id: &str) -> Result<Option<BridgeCenterRecord>>;
    fn get_bridge_center_by_code_impl(&self, code: &str) -> Result<Option<BridgeCenterRecord>>;
    fn list_bridge_centers_impl(
        &self,
        query: ListBridgeCentersQuery<'_>,
    ) -> Result<(Vec<BridgeCenterRecord>, i64)>;
    fn delete_bridge_center_impl(&self, center_id: &str) -> Result<i64>;
    fn upsert_bridge_center_account_impl(&self, record: &BridgeCenterAccountRecord) -> Result<()>;
    fn get_bridge_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>>;
    fn get_bridge_center_account_by_channel_account_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>>;
    fn list_bridge_center_accounts_impl(
        &self,
        query: ListBridgeCenterAccountsQuery<'_>,
    ) -> Result<(Vec<BridgeCenterAccountRecord>, i64)>;
    fn delete_bridge_center_account_impl(&self, center_account_id: &str) -> Result<i64>;
    fn delete_bridge_center_accounts_by_center_impl(&self, center_id: &str) -> Result<i64>;
    fn upsert_bridge_user_route_impl(&self, record: &BridgeUserRouteRecord) -> Result<()>;
    fn get_bridge_user_route_impl(&self, route_id: &str) -> Result<Option<BridgeUserRouteRecord>>;
    fn get_bridge_user_route_by_identity_impl(
        &self,
        center_account_id: &str,
        external_identity_key: &str,
    ) -> Result<Option<BridgeUserRouteRecord>>;
    fn list_bridge_user_routes_impl(
        &self,
        query: ListBridgeUserRoutesQuery<'_>,
    ) -> Result<(Vec<BridgeUserRouteRecord>, i64)>;
    fn delete_bridge_user_route_impl(&self, route_id: &str) -> Result<i64>;
    fn delete_bridge_user_routes_by_center_impl(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_user_routes_by_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<i64>;
    fn insert_bridge_delivery_log_impl(&self, record: &BridgeDeliveryLogRecord) -> Result<()>;
    fn list_bridge_delivery_logs_impl(
        &self,
        query: ListBridgeDeliveryLogsQuery<'_>,
    ) -> Result<Vec<BridgeDeliveryLogRecord>>;
    fn delete_bridge_delivery_logs_by_center_impl(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_delivery_logs_by_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<i64>;
    fn insert_bridge_route_audit_log_impl(&self, record: &BridgeRouteAuditLogRecord) -> Result<()>;
    fn list_bridge_route_audit_logs_impl(
        &self,
        query: ListBridgeRouteAuditLogsQuery<'_>,
    ) -> Result<Vec<BridgeRouteAuditLogRecord>>;
    fn delete_bridge_route_audit_logs_by_center_impl(&self, center_id: &str) -> Result<i64>;
    fn delete_bridge_route_audit_logs_by_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<i64>;
}

impl SqliteBridgeStorage for SqliteStorage {
    fn upsert_bridge_center_impl(&self, record: &BridgeCenterRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let settings_json = Self::json_to_string(&record.settings);
        conn.execute(
            "INSERT INTO bridge_centers (center_id, name, code, description, owner_user_id, status, default_preset_agent_name, target_unit_id, default_identity_strategy, username_policy, password_policy, settings_json, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(center_id) DO UPDATE SET name = excluded.name, code = excluded.code, description = excluded.description, owner_user_id = excluded.owner_user_id, status = excluded.status, default_preset_agent_name = excluded.default_preset_agent_name, target_unit_id = excluded.target_unit_id, default_identity_strategy = excluded.default_identity_strategy, username_policy = excluded.username_policy, password_policy = excluded.password_policy, settings_json = excluded.settings_json, updated_at = excluded.updated_at",
            params![
                record.center_id,
                record.name,
                record.code,
                record.description,
                record.owner_user_id,
                record.status,
                record.default_preset_agent_name,
                record.target_unit_id,
                record.default_identity_strategy,
                record.username_policy,
                record.password_policy,
                settings_json,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_bridge_center_impl(&self, center_id: &str) -> Result<Option<BridgeCenterRecord>> {
        self.ensure_initialized()?;
        let cleaned = center_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT center_id, name, code, description, owner_user_id, status, default_preset_agent_name, target_unit_id, default_identity_strategy, username_policy, password_policy, settings_json, created_at, updated_at \
                 FROM bridge_centers WHERE center_id = ?",
                params![cleaned],
                map_bridge_center_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_bridge_center_by_code_impl(&self, code: &str) -> Result<Option<BridgeCenterRecord>> {
        self.ensure_initialized()?;
        let cleaned = code.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT center_id, name, code, description, owner_user_id, status, default_preset_agent_name, target_unit_id, default_identity_strategy, username_policy, password_policy, settings_json, created_at, updated_at \
                 FROM bridge_centers WHERE code = ?",
                params![cleaned],
                map_bridge_center_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_bridge_centers_impl(
        &self,
        query: ListBridgeCentersQuery<'_>,
    ) -> Result<(Vec<BridgeCenterRecord>, i64)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(status) = query
            .status
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        if let Some(keyword) = query
            .keyword
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("(name LIKE ? OR code LIKE ? OR owner_user_id LIKE ?)".to_string());
            let like = format!("%{keyword}%");
            params_list.push(SqlValue::from(like.clone()));
            params_list.push(SqlValue::from(like.clone()));
            params_list.push(SqlValue::from(like));
        }
        let mut sql = "SELECT center_id, name, code, description, owner_user_id, status, default_preset_agent_name, target_unit_id, default_identity_strategy, username_policy, password_policy, settings_json, created_at, updated_at FROM bridge_centers".to_string();
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
        let rows = stmt.query_map(params_from_iter(params_list.iter()), map_bridge_center_row)?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        let mut count_sql = "SELECT COUNT(*) FROM bridge_centers".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_sql, count_params, |row| row.get(0))?;
        Ok((output, total))
    }

    fn delete_bridge_center_impl(&self, center_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_centers WHERE center_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_bridge_center_account_impl(&self, record: &BridgeCenterAccountRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let enabled = if record.enabled { 1 } else { 0 };
        let provider_caps_json = record.provider_caps.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO bridge_center_accounts (center_account_id, center_id, channel, account_id, enabled, default_preset_agent_name_override, identity_strategy, thread_strategy, reply_strategy, fallback_policy, provider_caps_json, status_reason, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(center_account_id) DO UPDATE SET center_id = excluded.center_id, channel = excluded.channel, account_id = excluded.account_id, enabled = excluded.enabled, default_preset_agent_name_override = excluded.default_preset_agent_name_override, identity_strategy = excluded.identity_strategy, thread_strategy = excluded.thread_strategy, reply_strategy = excluded.reply_strategy, fallback_policy = excluded.fallback_policy, provider_caps_json = excluded.provider_caps_json, status_reason = excluded.status_reason, updated_at = excluded.updated_at",
            params![
                record.center_account_id,
                record.center_id,
                record.channel,
                record.account_id,
                enabled,
                record.default_preset_agent_name_override,
                record.identity_strategy,
                record.thread_strategy,
                record.reply_strategy,
                record.fallback_policy,
                provider_caps_json,
                record.status_reason,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_bridge_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned = center_account_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT center_account_id, center_id, channel, account_id, enabled, default_preset_agent_name_override, identity_strategy, thread_strategy, reply_strategy, fallback_policy, provider_caps_json, status_reason, created_at, updated_at \
                 FROM bridge_center_accounts WHERE center_account_id = ?",
                params![cleaned],
                map_bridge_center_account_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_bridge_center_account_by_channel_account_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<Option<BridgeCenterAccountRecord>> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT center_account_id, center_id, channel, account_id, enabled, default_preset_agent_name_override, identity_strategy, thread_strategy, reply_strategy, fallback_policy, provider_caps_json, status_reason, created_at, updated_at \
                 FROM bridge_center_accounts WHERE channel = ? AND account_id = ?",
                params![cleaned_channel, cleaned_account],
                map_bridge_center_account_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_bridge_center_accounts_impl(
        &self,
        query: ListBridgeCenterAccountsQuery<'_>,
    ) -> Result<(Vec<BridgeCenterAccountRecord>, i64)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(center_id) = query
            .center_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("center_id = ?".to_string());
            params_list.push(SqlValue::from(center_id.to_string()));
        }
        if let Some(channel) = query
            .channel
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(account_id) = query
            .account_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("account_id = ?".to_string());
            params_list.push(SqlValue::from(account_id.to_string()));
        }
        if let Some(enabled) = query.enabled {
            filters.push("enabled = ?".to_string());
            params_list.push(SqlValue::from(if enabled { 1 } else { 0 }));
        }
        let mut sql = "SELECT center_account_id, center_id, channel, account_id, enabled, default_preset_agent_name_override, identity_strategy, thread_strategy, reply_strategy, fallback_policy, provider_caps_json, status_reason, created_at, updated_at FROM bridge_center_accounts".to_string();
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
            map_bridge_center_account_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        let mut count_sql = "SELECT COUNT(*) FROM bridge_center_accounts".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_sql, count_params, |row| row.get(0))?;
        Ok((output, total))
    }

    fn delete_bridge_center_account_impl(&self, center_account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_account_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_center_accounts WHERE center_account_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn delete_bridge_center_accounts_by_center_impl(&self, center_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_center_accounts WHERE center_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn upsert_bridge_user_route_impl(&self, record: &BridgeUserRouteRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let external_profile_json = record.external_profile.as_ref().map(Self::json_to_string);
        let user_created = if record.user_created { 1 } else { 0 };
        let agent_created = if record.agent_created { 1 } else { 0 };
        conn.execute(
            "INSERT INTO bridge_user_routes (route_id, center_id, center_account_id, channel, account_id, external_identity_key, external_user_key, external_display_name, external_peer_id, external_sender_id, external_thread_id, external_profile_json, wunder_user_id, agent_id, agent_name, user_created, agent_created, status, last_session_id, last_error, first_seen_at, last_seen_at, last_inbound_at, last_outbound_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(center_account_id, external_identity_key) DO UPDATE SET center_id = excluded.center_id, channel = excluded.channel, account_id = excluded.account_id, external_user_key = excluded.external_user_key, external_display_name = excluded.external_display_name, external_peer_id = excluded.external_peer_id, external_sender_id = excluded.external_sender_id, external_thread_id = excluded.external_thread_id, external_profile_json = excluded.external_profile_json, wunder_user_id = excluded.wunder_user_id, agent_id = excluded.agent_id, agent_name = excluded.agent_name, user_created = excluded.user_created, agent_created = excluded.agent_created, status = excluded.status, last_session_id = excluded.last_session_id, last_error = excluded.last_error, last_seen_at = excluded.last_seen_at, last_inbound_at = excluded.last_inbound_at, last_outbound_at = excluded.last_outbound_at, updated_at = excluded.updated_at",
            params![
                record.route_id,
                record.center_id,
                record.center_account_id,
                record.channel,
                record.account_id,
                record.external_identity_key,
                record.external_user_key,
                record.external_display_name,
                record.external_peer_id,
                record.external_sender_id,
                record.external_thread_id,
                external_profile_json,
                record.wunder_user_id,
                record.agent_id,
                record.agent_name,
                user_created,
                agent_created,
                record.status,
                record.last_session_id,
                record.last_error,
                record.first_seen_at,
                record.last_seen_at,
                record.last_inbound_at,
                record.last_outbound_at,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_bridge_user_route_impl(&self, route_id: &str) -> Result<Option<BridgeUserRouteRecord>> {
        self.ensure_initialized()?;
        let cleaned = route_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT route_id, center_id, center_account_id, channel, account_id, external_identity_key, external_user_key, external_display_name, external_peer_id, external_sender_id, external_thread_id, external_profile_json, wunder_user_id, agent_id, agent_name, user_created, agent_created, status, last_session_id, last_error, first_seen_at, last_seen_at, last_inbound_at, last_outbound_at, created_at, updated_at \
                 FROM bridge_user_routes WHERE route_id = ?",
                params![cleaned],
                map_bridge_user_route_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_bridge_user_route_by_identity_impl(
        &self,
        center_account_id: &str,
        external_identity_key: &str,
    ) -> Result<Option<BridgeUserRouteRecord>> {
        self.ensure_initialized()?;
        let cleaned_center_account = center_account_id.trim();
        let cleaned_identity = external_identity_key.trim();
        if cleaned_center_account.is_empty() || cleaned_identity.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT route_id, center_id, center_account_id, channel, account_id, external_identity_key, external_user_key, external_display_name, external_peer_id, external_sender_id, external_thread_id, external_profile_json, wunder_user_id, agent_id, agent_name, user_created, agent_created, status, last_session_id, last_error, first_seen_at, last_seen_at, last_inbound_at, last_outbound_at, created_at, updated_at \
                 FROM bridge_user_routes WHERE center_account_id = ? AND external_identity_key = ?",
                params![cleaned_center_account, cleaned_identity],
                map_bridge_user_route_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_bridge_user_routes_impl(
        &self,
        query: ListBridgeUserRoutesQuery<'_>,
    ) -> Result<(Vec<BridgeUserRouteRecord>, i64)> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(center_id) = query
            .center_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("center_id = ?".to_string());
            params_list.push(SqlValue::from(center_id.to_string()));
        }
        if let Some(center_account_id) = query
            .center_account_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("center_account_id = ?".to_string());
            params_list.push(SqlValue::from(center_account_id.to_string()));
        }
        if let Some(channel) = query
            .channel
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("channel = ?".to_string());
            params_list.push(SqlValue::from(channel.to_string()));
        }
        if let Some(account_id) = query
            .account_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("account_id = ?".to_string());
            params_list.push(SqlValue::from(account_id.to_string()));
        }
        if let Some(status) = query
            .status
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        if let Some(wunder_user_id) = query
            .wunder_user_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("wunder_user_id = ?".to_string());
            params_list.push(SqlValue::from(wunder_user_id.to_string()));
        }
        if let Some(agent_id) = query
            .agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("agent_id = ?".to_string());
            params_list.push(SqlValue::from(agent_id.to_string()));
        }
        if let Some(identity_key) = query
            .external_identity_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("external_identity_key = ?".to_string());
            params_list.push(SqlValue::from(identity_key.to_string()));
        }
        if let Some(keyword) = query
            .keyword
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("(external_display_name LIKE ? OR external_identity_key LIKE ? OR wunder_user_id LIKE ? OR agent_name LIKE ? OR agent_id LIKE ?)".to_string());
            let like = format!("%{keyword}%");
            params_list.push(SqlValue::from(like.clone()));
            params_list.push(SqlValue::from(like.clone()));
            params_list.push(SqlValue::from(like.clone()));
            params_list.push(SqlValue::from(like.clone()));
            params_list.push(SqlValue::from(like));
        }
        let mut sql = "SELECT route_id, center_id, center_account_id, channel, account_id, external_identity_key, external_user_key, external_display_name, external_peer_id, external_sender_id, external_thread_id, external_profile_json, wunder_user_id, agent_id, agent_name, user_created, agent_created, status, last_session_id, last_error, first_seen_at, last_seen_at, last_inbound_at, last_outbound_at, created_at, updated_at FROM bridge_user_routes".to_string();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(" ORDER BY last_seen_at DESC, updated_at DESC");
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
            map_bridge_user_route_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        let mut count_sql = "SELECT COUNT(*) FROM bridge_user_routes".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_sql, count_params, |row| row.get(0))?;
        Ok((output, total))
    }

    fn delete_bridge_user_route_impl(&self, route_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = route_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_user_routes WHERE route_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn delete_bridge_user_routes_by_center_impl(&self, center_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_user_routes WHERE center_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn delete_bridge_user_routes_by_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_account_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_user_routes WHERE center_account_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn insert_bridge_delivery_log_impl(&self, record: &BridgeDeliveryLogRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let payload_json = record.payload.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO bridge_delivery_logs (delivery_id, center_id, center_account_id, route_id, direction, stage, provider_message_id, session_id, status, summary, payload_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.delivery_id,
                record.center_id,
                record.center_account_id,
                record.route_id,
                record.direction,
                record.stage,
                record.provider_message_id,
                record.session_id,
                record.status,
                record.summary,
                payload_json,
                record.created_at
            ],
        )?;
        Ok(())
    }

    fn list_bridge_delivery_logs_impl(
        &self,
        query: ListBridgeDeliveryLogsQuery<'_>,
    ) -> Result<Vec<BridgeDeliveryLogRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(center_id) = query
            .center_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("center_id = ?".to_string());
            params_list.push(SqlValue::from(center_id.to_string()));
        }
        if let Some(center_account_id) = query
            .center_account_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("center_account_id = ?".to_string());
            params_list.push(SqlValue::from(center_account_id.to_string()));
        }
        if let Some(route_id) = query
            .route_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("route_id = ?".to_string());
            params_list.push(SqlValue::from(route_id.to_string()));
        }
        if let Some(direction) = query
            .direction
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("direction = ?".to_string());
            params_list.push(SqlValue::from(direction.to_string()));
        }
        if let Some(status) = query
            .status
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        let mut sql = "SELECT delivery_id, center_id, center_account_id, route_id, direction, stage, provider_message_id, session_id, status, summary, payload_json, created_at FROM bridge_delivery_logs".to_string();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC");
        let limit_value = if query.limit <= 0 {
            100
        } else {
            query.limit.min(500)
        };
        params_list.push(SqlValue::from(limit_value));
        sql.push_str(" LIMIT ?");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params_from_iter(params_list.iter()),
            map_bridge_delivery_log_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_bridge_delivery_logs_by_center_impl(&self, center_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_delivery_logs WHERE center_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn delete_bridge_delivery_logs_by_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_account_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_delivery_logs WHERE center_account_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn insert_bridge_route_audit_log_impl(&self, record: &BridgeRouteAuditLogRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let detail_json = record.detail.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO bridge_route_audit_logs (audit_id, center_id, route_id, actor_type, actor_id, action, detail_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.audit_id,
                record.center_id,
                record.route_id,
                record.actor_type,
                record.actor_id,
                record.action,
                detail_json,
                record.created_at
            ],
        )?;
        Ok(())
    }

    fn list_bridge_route_audit_logs_impl(
        &self,
        query: ListBridgeRouteAuditLogsQuery<'_>,
    ) -> Result<Vec<BridgeRouteAuditLogRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(center_id) = query
            .center_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("center_id = ?".to_string());
            params_list.push(SqlValue::from(center_id.to_string()));
        }
        if let Some(route_id) = query
            .route_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            filters.push("route_id = ?".to_string());
            params_list.push(SqlValue::from(route_id.to_string()));
        }
        let mut sql = "SELECT audit_id, center_id, route_id, actor_type, actor_id, action, detail_json, created_at FROM bridge_route_audit_logs".to_string();
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC");
        let limit_value = if query.limit <= 0 {
            100
        } else {
            query.limit.min(500)
        };
        params_list.push(SqlValue::from(limit_value));
        sql.push_str(" LIMIT ?");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params_from_iter(params_list.iter()),
            map_bridge_route_audit_log_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_bridge_route_audit_logs_by_center_impl(&self, center_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_route_audit_logs WHERE center_id = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }

    fn delete_bridge_route_audit_logs_by_center_account_impl(
        &self,
        center_account_id: &str,
    ) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = center_account_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM bridge_route_audit_logs \
             WHERE route_id IN (SELECT route_id FROM bridge_user_routes WHERE center_account_id = ?)",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }
}

fn map_bridge_center_row(row: &Row<'_>) -> rusqlite::Result<BridgeCenterRecord> {
    let settings_json: String = row.get(11)?;
    Ok(BridgeCenterRecord {
        center_id: row.get(0)?,
        name: row.get(1)?,
        code: row.get(2)?,
        description: row.get(3)?,
        owner_user_id: row.get(4)?,
        status: row.get(5)?,
        default_preset_agent_name: row.get(6)?,
        target_unit_id: row.get(7)?,
        default_identity_strategy: row.get(8)?,
        username_policy: row.get(9)?,
        password_policy: row.get(10)?,
        settings: SqliteStorage::json_from_str(&settings_json).unwrap_or(Value::Null),
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn map_bridge_center_account_row(row: &Row<'_>) -> rusqlite::Result<BridgeCenterAccountRecord> {
    let provider_caps_json: Option<String> = row.get(10)?;
    Ok(BridgeCenterAccountRecord {
        center_account_id: row.get(0)?,
        center_id: row.get(1)?,
        channel: row.get(2)?,
        account_id: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        default_preset_agent_name_override: row.get(5)?,
        identity_strategy: row.get(6)?,
        thread_strategy: row.get(7)?,
        reply_strategy: row.get(8)?,
        fallback_policy: row.get(9)?,
        provider_caps: provider_caps_json.and_then(|value| SqliteStorage::json_from_str(&value)),
        status_reason: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn map_bridge_user_route_row(row: &Row<'_>) -> rusqlite::Result<BridgeUserRouteRecord> {
    let external_profile_json: Option<String> = row.get(11)?;
    Ok(BridgeUserRouteRecord {
        route_id: row.get(0)?,
        center_id: row.get(1)?,
        center_account_id: row.get(2)?,
        channel: row.get(3)?,
        account_id: row.get(4)?,
        external_identity_key: row.get(5)?,
        external_user_key: row.get(6)?,
        external_display_name: row.get(7)?,
        external_peer_id: row.get(8)?,
        external_sender_id: row.get(9)?,
        external_thread_id: row.get(10)?,
        external_profile: external_profile_json
            .and_then(|value| SqliteStorage::json_from_str(&value)),
        wunder_user_id: row.get(12)?,
        agent_id: row.get(13)?,
        agent_name: row.get(14)?,
        user_created: row.get::<_, i64>(15)? != 0,
        agent_created: row.get::<_, i64>(16)? != 0,
        status: row.get(17)?,
        last_session_id: row.get(18)?,
        last_error: row.get(19)?,
        first_seen_at: row.get(20)?,
        last_seen_at: row.get(21)?,
        last_inbound_at: row.get(22)?,
        last_outbound_at: row.get(23)?,
        created_at: row.get(24)?,
        updated_at: row.get(25)?,
    })
}

fn map_bridge_delivery_log_row(row: &Row<'_>) -> rusqlite::Result<BridgeDeliveryLogRecord> {
    let payload_json: Option<String> = row.get(10)?;
    Ok(BridgeDeliveryLogRecord {
        delivery_id: row.get(0)?,
        center_id: row.get(1)?,
        center_account_id: row.get(2)?,
        route_id: row.get(3)?,
        direction: row.get(4)?,
        stage: row.get(5)?,
        provider_message_id: row.get(6)?,
        session_id: row.get(7)?,
        status: row.get(8)?,
        summary: row.get(9)?,
        payload: payload_json.and_then(|value| SqliteStorage::json_from_str(&value)),
        created_at: row.get(11)?,
    })
}

fn map_bridge_route_audit_log_row(row: &Row<'_>) -> rusqlite::Result<BridgeRouteAuditLogRecord> {
    let detail_json: Option<String> = row.get(6)?;
    Ok(BridgeRouteAuditLogRecord {
        audit_id: row.get(0)?,
        center_id: row.get(1)?,
        route_id: row.get(2)?,
        actor_type: row.get(3)?,
        actor_id: row.get(4)?,
        action: row.get(5)?,
        detail: detail_json.and_then(|value| SqliteStorage::json_from_str(&value)),
        created_at: row.get(7)?,
    })
}
