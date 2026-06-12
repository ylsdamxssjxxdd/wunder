use super::PostgresStorage;
use crate::storage::{
    GatewayClientRecord, GatewayNodeRecord, GatewayNodeTokenRecord, StorageBackend,
};
use anyhow::Result;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

pub(super) trait PostgresGatewayStorage {
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

impl PostgresGatewayStorage for PostgresStorage {
    fn upsert_gateway_client_impl(&self, record: &GatewayClientRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
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
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
             ON CONFLICT(connection_id) DO UPDATE SET role = EXCLUDED.role, user_id = EXCLUDED.user_id, node_id = EXCLUDED.node_id, scopes = EXCLUDED.scopes, \
             caps = EXCLUDED.caps, commands = EXCLUDED.commands, client_info = EXCLUDED.client_info, status = EXCLUDED.status, last_seen_at = EXCLUDED.last_seen_at, \
             disconnected_at = EXCLUDED.disconnected_at",
            &[
                &record.connection_id,
                &record.role,
                &record.user_id,
                &record.node_id,
                &scopes,
                &caps,
                &commands,
                &client_info,
                &record.status,
                &record.connected_at,
                &record.last_seen_at,
                &record.disconnected_at,
            ],
        )?;
        Ok(())
    }

    fn list_gateway_clients_impl(&self, status: Option<&str>) -> Result<Vec<GatewayClientRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = "SELECT connection_id, role, user_id, node_id, scopes, caps, commands, client_info, status, connected_at, last_seen_at, disconnected_at FROM gateway_clients".to_string();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
            query.push_str(" WHERE status = $1");
            params.push(Box::new(status.to_string()));
        }
        query.push_str(" ORDER BY last_seen_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_gateway_client_row(&row));
        }
        Ok(output)
    }

    fn upsert_gateway_node_impl(&self, record: &GatewayNodeRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
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
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
             ON CONFLICT(node_id) DO UPDATE SET name = EXCLUDED.name, device_fingerprint = EXCLUDED.device_fingerprint, status = EXCLUDED.status, caps = EXCLUDED.caps, \
             commands = EXCLUDED.commands, permissions = EXCLUDED.permissions, metadata = EXCLUDED.metadata, updated_at = EXCLUDED.updated_at, last_seen_at = EXCLUDED.last_seen_at",
            &[
                &record.node_id,
                &record.name,
                &record.device_fingerprint,
                &record.status,
                &caps,
                &commands,
                &permissions,
                &metadata,
                &record.created_at,
                &record.updated_at,
                &record.last_seen_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes WHERE node_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_gateway_node_row(&row)))
    }

    fn list_gateway_nodes_impl(&self, status: Option<&str>) -> Result<Vec<GatewayNodeRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query = "SELECT node_id, name, device_fingerprint, status, caps, commands, permissions, metadata, created_at, updated_at, last_seen_at FROM gateway_nodes".to_string();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
            query.push_str(" WHERE status = $1");
            params.push(Box::new(status.to_string()));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_gateway_node_row(&row));
        }
        Ok(output)
    }

    fn upsert_gateway_node_token_impl(&self, record: &GatewayNodeTokenRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO gateway_node_tokens (token, node_id, status, created_at, updated_at, last_used_at) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT(token) DO UPDATE SET node_id = EXCLUDED.node_id, status = EXCLUDED.status, updated_at = EXCLUDED.updated_at, last_used_at = EXCLUDED.last_used_at",
            &[
                &record.token,
                &record.node_id,
                &record.status,
                &record.created_at,
                &record.updated_at,
                &record.last_used_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens WHERE token = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_gateway_node_token_row(&row)))
    }

    fn list_gateway_node_tokens_impl(
        &self,
        node_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<GatewayNodeTokenRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut query =
            "SELECT token, node_id, status, created_at, updated_at, last_used_at FROM gateway_node_tokens"
                .to_string();
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        push_optional_filter(&mut filters, &mut params, "node_id", node_id);
        push_optional_filter(&mut filters, &mut params, "status", status);
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(map_gateway_node_token_row(&row));
        }
        Ok(output)
    }

    fn delete_gateway_node_token_impl(&self, token: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = token.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM gateway_node_tokens WHERE token = $1",
            &[&cleaned],
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
        filters.push(format!("{column} = ${}", params.len() + 1));
        params.push(Box::new(value.to_string()));
    }
}

fn map_gateway_client_row(row: &Row) -> GatewayClientRecord {
    let scopes: Option<String> = row.get(4);
    let caps: Option<String> = row.get(5);
    let commands: Option<String> = row.get(6);
    let client_info: Option<String> = row.get(7);
    GatewayClientRecord {
        connection_id: row.get(0),
        role: row.get(1),
        user_id: row.get(2),
        node_id: row.get(3),
        scopes: PostgresStorage::parse_string_list(scopes),
        caps: PostgresStorage::parse_string_list(caps),
        commands: PostgresStorage::parse_string_list(commands),
        client_info: client_info
            .as_deref()
            .and_then(PostgresStorage::json_from_str),
        status: row.get(8),
        connected_at: row.get(9),
        last_seen_at: row.get(10),
        disconnected_at: row.get(11),
    }
}

fn map_gateway_node_row(row: &Row) -> GatewayNodeRecord {
    let caps: Option<String> = row.get(4);
    let commands: Option<String> = row.get(5);
    let permissions: Option<String> = row.get(6);
    let metadata: Option<String> = row.get(7);
    GatewayNodeRecord {
        node_id: row.get(0),
        name: row.get(1),
        device_fingerprint: row.get(2),
        status: row.get(3),
        caps: PostgresStorage::parse_string_list(caps),
        commands: PostgresStorage::parse_string_list(commands),
        permissions: permissions
            .as_deref()
            .and_then(PostgresStorage::json_from_str),
        metadata: metadata.as_deref().and_then(PostgresStorage::json_from_str),
        created_at: row.get(8),
        updated_at: row.get(9),
        last_seen_at: row.get(10),
    }
}

fn map_gateway_node_token_row(row: &Row) -> GatewayNodeTokenRecord {
    GatewayNodeTokenRecord {
        token: row.get(0),
        node_id: row.get(1),
        status: row.get(2),
        created_at: row.get(3),
        updated_at: row.get(4),
        last_used_at: row.get(5),
    }
}
