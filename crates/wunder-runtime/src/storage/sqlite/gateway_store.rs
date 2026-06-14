use super::SqliteStorage;
use crate::storage::{
    GatewayClientRecord, GatewayNodeRecord, GatewayNodeTokenRecord, StorageBackend,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

pub(super) trait SqliteGatewayStorage {
    fn upsert_gateway_client_impl(&self, record: &GatewayClientRecord) -> Result<()>;
    fn list_gateway_clients_impl(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>>;
    fn upsert_gateway_node_impl(&self, record: &GatewayNodeRecord) -> Result<()>;
    fn get_gateway_node_impl(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>>;
    fn list_gateway_nodes_impl(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>>;
    fn upsert_gateway_node_token_impl(&self, record: &GatewayNodeTokenRecord) -> Result<()>;
    fn get_gateway_node_token_impl(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>>;
    fn list_gateway_node_tokens_impl(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>>;
    fn delete_gateway_node_token_impl(&self, token: &str) -> Result<i64>;
}

impl SqliteGatewayStorage for SqliteStorage {
    fn upsert_gateway_client_impl(&self, record: &GatewayClientRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let scopes = if record.scopes.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.scopes))
        };
        let caps = if record.caps.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.caps))
        };
        let commands = if record.commands.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.commands))
        };
        let client_info = record.client_info.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO gateway_clients (connection_id, role, user_id, node_id, scopes, caps, commands, client_info, status, connected_at, last_seen_at, disconnected_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(connection_id) DO UPDATE SET role = excluded.role, user_id = excluded.user_id, node_id = excluded.node_id, scopes = excluded.scopes, \
             caps = excluded.caps, commands = excluded.commands, client_info = excluded.client_info, status = excluded.status, last_seen_at = excluded.last_seen_at, \
             disconnected_at = excluded.disconnected_at",
            params![
                record.connection_id,
                record.role,
                record.user_id,
                record.node_id,
                scopes,
                caps,
                commands,
                client_info,
                record.status,
                record.connected_at,
                record.last_seen_at,
                record.disconnected_at
            ],
        )?;
        Ok(())
    }

    fn list_gateway_clients_impl(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = "SELECT connection_id, role, user_id, node_id, scopes, caps, commands, client_info, status, connected_at, last_seen_at, disconnected_at FROM gateway_clients".to_string();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
            query.push_str(" WHERE status = ?");
            params_list.push(SqlValue::from(status.to_string()));
        }
        query.push_str(" ORDER BY last_seen_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), map_gateway_client_row)?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn upsert_gateway_node_impl(&self, record: &GatewayNodeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let caps = if record.caps.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.caps))
        };
        let commands = if record.commands.is_empty() {
            None
        } else {
            Some(Self::string_list_to_json(&record.commands))
        };
        let permissions = record.permissions.as_ref().map(Self::json_to_string);
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO gateway_nodes (node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(node_id) DO UPDATE SET name = excluded.name, device_fingerprint = excluded.device_fingerprint, status = excluded.status, caps = excluded.caps, \
             commands = excluded.commands, permissions = excluded.permissions, metadata = excluded.metadata, updated_at = excluded.updated_at, last_seen_at = excluded.last_seen_at",
            params![
                record.node_id,
                record.name,
                record.device_fingerprint,
                record.status,
                caps,
                commands,
                permissions,
                metadata,
                record.created_at,
                record.updated_at,
                record.last_seen_at
            ],
        )?;
        Ok(())
    }

    fn get_gateway_node_impl(&self, node_id: &str) -> Result<Option<GatewayNodeRecord>> {
        self.ensure_initialized()?;
        let cleaned = node_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes WHERE node_id = ?",
                params![cleaned],
                map_gateway_node_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_gateway_nodes_impl(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query = "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes".to_string();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
            query.push_str(" WHERE status = ?");
            params_list.push(SqlValue::from(status.to_string()));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), map_gateway_node_row)?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn upsert_gateway_node_token_impl(&self, record: &GatewayNodeTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        conn.execute(
            "INSERT INTO gateway_node_tokens (token, node_id, status, created_at, updated_at, last_used_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(token) DO UPDATE SET node_id = excluded.node_id, status = excluded.status, updated_at = excluded.updated_at, last_used_at = excluded.last_used_at",
            params![
                record.token,
                record.node_id,
                record.status,
                record.created_at,
                record.updated_at,
                record.last_used_at
            ],
        )?;
        Ok(())
    }

    fn get_gateway_node_token_impl(&self, token: &str) -> Result<Option<GatewayNodeTokenRecord>> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens WHERE token = ?",
                params![cleaned],
                map_gateway_node_token_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_gateway_node_tokens_impl(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let mut query =
            "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens"
                .to_string();
        let mut filters = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(node_id) = node_id.map(str::trim).filter(|value| !value.is_empty()) {
            filters.push("node_id = ?".to_string());
            params_list.push(SqlValue::from(node_id.to_string()));
        }
        if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
            filters.push("status = ?".to_string());
            params_list.push(SqlValue::from(status.to_string()));
        }
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(
            params_from_iter(params_list.iter()),
            map_gateway_node_token_row,
        )?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn delete_gateway_node_token_impl(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM gateway_node_tokens WHERE token = ?",
            params![cleaned],
        )?;
        Ok(affected as i64)
    }
}

fn map_gateway_client_row(row: &Row<'_>) -> rusqlite::Result<GatewayClientRecord> {
    let scopes: Option<String> = row.get(4)?;
    let caps: Option<String> = row.get(5)?;
    let commands: Option<String> = row.get(6)?;
    let client_info: Option<String> = row.get(7)?;
    Ok(GatewayClientRecord {
        connection_id: row.get(0)?,
        role: row.get(1)?,
        user_id: row.get(2)?,
        node_id: row.get(3)?,
        scopes: SqliteStorage::parse_string_list(scopes),
        caps: SqliteStorage::parse_string_list(caps),
        commands: SqliteStorage::parse_string_list(commands),
        client_info: client_info
            .as_deref()
            .and_then(SqliteStorage::json_from_str),
        status: row.get(8)?,
        connected_at: row.get(9)?,
        last_seen_at: row.get(10)?,
        disconnected_at: row.get(11)?,
    })
}

fn map_gateway_node_row(row: &Row<'_>) -> rusqlite::Result<GatewayNodeRecord> {
    let caps: Option<String> = row.get(4)?;
    let commands: Option<String> = row.get(5)?;
    let permissions: Option<String> = row.get(6)?;
    let metadata: Option<String> = row.get(7)?;
    Ok(GatewayNodeRecord {
        node_id: row.get(0)?,
        name: row.get(1)?,
        device_fingerprint: row.get(2)?,
        status: row.get(3)?,
        caps: SqliteStorage::parse_string_list(caps),
        commands: SqliteStorage::parse_string_list(commands),
        permissions: permissions
            .as_deref()
            .and_then(SqliteStorage::json_from_str),
        metadata: metadata.as_deref().and_then(SqliteStorage::json_from_str),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_seen_at: row.get(10)?,
    })
}

fn map_gateway_node_token_row(row: &Row<'_>) -> rusqlite::Result<GatewayNodeTokenRecord> {
    Ok(GatewayNodeTokenRecord {
        token: row.get(0)?,
        node_id: row.get(1)?,
        status: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        last_used_at: row.get(5)?,
    })
}
