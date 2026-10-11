use super::PostgresStorage;
use crate::storage::{
    CloudDeviceInterlinkPatch, InterlinkApprovalRecord, InterlinkAuditRecord,
    InterlinkChannelRecord, InterlinkCommandRecord, InterlinkShadowRecord,
    ListInterlinkAuditQuery, ListInterlinkCommandsQuery, StorageLifecycle,
};
use anyhow::Result;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// Rows deleted per pass of the shadow retention sweep.
const SHADOW_CLEANUP_PAGE: i64 = 200;
/// Hard ceiling of one sweep call, mirroring the SQLite side.
const SHADOW_CLEANUP_MAX_ROWS: i64 = 5_000;
/// Page bounds for the ledger and audit sweeps, mirroring the SQLite side.
const LEDGER_CLEANUP_PAGE: i64 = 500;
const LEDGER_CLEANUP_MAX_ROWS: i64 = 20_000;

pub(super) trait PostgresInterlinkStorage {
    fn update_cloud_device_interlink_impl(
        &self,
        device_id: &str,
        patch: &CloudDeviceInterlinkPatch,
    ) -> Result<()>;
    fn upsert_interlink_channel_impl(&self, record: &InterlinkChannelRecord) -> Result<()>;
    fn get_interlink_channel_impl(
        &self,
        channel_id: &str,
    ) -> Result<Option<InterlinkChannelRecord>>;
    fn close_interlink_channel_impl(&self, channel_id: &str, closed_reason: &str) -> Result<()>;
    fn list_interlink_channels_impl(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<InterlinkChannelRecord>, i64)>;
    fn upsert_interlink_shadow_impl(&self, record: &InterlinkShadowRecord) -> Result<()>;
    fn get_interlink_shadow_impl(&self, device_id: &str)
        -> Result<Option<InterlinkShadowRecord>>;
    fn get_interlink_shadow_revision_impl(&self, device_id: &str) -> Result<i64>;
    fn delete_interlink_shadow_impl(&self, device_id: &str) -> Result<()>;
    fn cleanup_interlink_shadows_impl(&self, retention_days: u32, max_rows: i64) -> Result<u64>;
    fn insert_interlink_command_impl(&self, record: &InterlinkCommandRecord) -> Result<bool>;
    fn update_interlink_command_status_impl(
        &self,
        command_id: &str,
        status: &str,
        acked_at: Option<f64>,
        finished_at: Option<f64>,
        error_code: Option<&str>,
        error_summary: Option<&str>,
    ) -> Result<()>;
    fn set_interlink_command_approval_impl(
        &self,
        command_id: &str,
        approval_state: &str,
    ) -> Result<()>;
    fn get_interlink_command_impl(
        &self,
        command_id: &str,
    ) -> Result<Option<InterlinkCommandRecord>>;
    fn list_interlink_commands_impl(
        &self,
        query: ListInterlinkCommandsQuery<'_>,
    ) -> Result<(Vec<InterlinkCommandRecord>, i64)>;
    fn cleanup_interlink_commands_impl(&self, retention_days: u32) -> Result<u64>;
    fn insert_interlink_approval_impl(&self, record: &InterlinkApprovalRecord) -> Result<()>;
    fn decide_interlink_approval_impl(
        &self,
        approval_id: &str,
        state: &str,
        decided_by: &str,
        decided_at: f64,
    ) -> Result<()>;
    fn get_interlink_approval_impl(
        &self,
        approval_id: &str,
    ) -> Result<Option<InterlinkApprovalRecord>>;
    fn insert_interlink_audit_impl(&self, record: &InterlinkAuditRecord) -> Result<()>;
    fn list_interlink_audit_impl(
        &self,
        query: ListInterlinkAuditQuery<'_>,
    ) -> Result<(Vec<InterlinkAuditRecord>, i64)>;
    fn cleanup_interlink_audit_impl(&self, retention_days: u32) -> Result<u64>;
}

fn map_channel_row(row: &Row) -> InterlinkChannelRecord {
    InterlinkChannelRecord {
        channel_id: row.get(0),
        device_id: row.get(1),
        user_id: row.get::<_, Option<String>>(2).unwrap_or_default(),
        client: row.get::<_, Option<String>>(3).unwrap_or_default(),
        instance_id: row
            .get::<_, Option<String>>(4)
            .unwrap_or_else(|| "local".to_string()),
        protocol_version: row.get::<_, Option<i64>>(5).unwrap_or(1),
        caps: row.get(6),
        connected_at: row.get::<_, Option<f64>>(7).unwrap_or(0.0),
        last_seen_at: row.get::<_, Option<f64>>(8).unwrap_or(0.0),
        rtt_ms: row.get(9),
        resumed_count: row.get::<_, Option<i64>>(10).unwrap_or(0),
        closed_reason: row.get(11),
    }
}

fn map_shadow_row(row: &Row) -> InterlinkShadowRecord {
    InterlinkShadowRecord {
        device_id: row.get(0),
        user_id: row.get::<_, Option<String>>(1).unwrap_or_default(),
        revision: row.get::<_, Option<i64>>(2).unwrap_or(0),
        summary: row.get(3),
        threads: row.get(4),
        tasks: row.get(5),
        workspace: row.get(6),
        synced_at: row.get::<_, Option<f64>>(7).unwrap_or(0.0),
    }
}

fn map_command_row(row: &Row) -> InterlinkCommandRecord {
    InterlinkCommandRecord {
        command_id: row.get(0),
        direction: row.get::<_, Option<String>>(1).unwrap_or_default(),
        actor_user_id: row.get::<_, Option<String>>(2).unwrap_or_default(),
        from_node: row.get::<_, Option<String>>(3).unwrap_or_default(),
        to_node: row.get::<_, Option<String>>(4).unwrap_or_default(),
        kind: row.get::<_, Option<String>>(5).unwrap_or_default(),
        args_digest: row.get(6),
        approval_state: row
            .get::<_, Option<String>>(7)
            .unwrap_or_else(|| "none".to_string()),
        status: row
            .get::<_, Option<String>>(8)
            .unwrap_or_else(|| "issued".to_string()),
        created_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
        acked_at: row.get(10),
        finished_at: row.get(11),
        error_code: row.get(12),
        error_summary: row.get(13),
    }
}

fn map_approval_row(row: &Row) -> InterlinkApprovalRecord {
    InterlinkApprovalRecord {
        approval_id: row.get(0),
        command_id: row.get::<_, Option<String>>(1).unwrap_or_default(),
        device_id: row.get::<_, Option<String>>(2).unwrap_or_default(),
        user_id: row.get::<_, Option<String>>(3).unwrap_or_default(),
        prompt: row.get::<_, Option<String>>(4).unwrap_or_default(),
        risk_level: row.get::<_, Option<String>>(5).unwrap_or_default(),
        state: row
            .get::<_, Option<String>>(6)
            .unwrap_or_else(|| "pending".to_string()),
        decided_by: row.get(7),
        decided_at: row.get(8),
        expires_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
    }
}

const CHANNEL_COLUMNS: &str =
    "channel_id, device_id, user_id, client, instance_id, protocol_version, caps, connected_at, last_seen_at, rtt_ms, resumed_count, closed_reason";
const COMMAND_COLUMNS: &str =
    "command_id, direction, actor_user_id, from_node, to_node, kind, args_digest, approval_state, status, created_at, acked_at, finished_at, error_code, error_summary";

impl PostgresInterlinkStorage for PostgresStorage {
    fn update_cloud_device_interlink_impl(
        &self,
        device_id: &str,
        patch: &CloudDeviceInterlinkPatch,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut sets = Vec::new();
        let mut values: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        let mut idx = 1_usize;
        macro_rules! push_str {
            ($column:expr, $value:expr) => {{
                values.push(Box::new($value.clone()));
                sets.push(format!("{} = ${}", $column, idx));
                idx += 1;
            }};
        }
        if let Some(secret_hash) = &patch.node_secret_hash {
            push_str!("node_secret_hash", secret_hash);
        }
        if patch.secret_version > 0 {
            values.push(Box::new(patch.secret_version));
            sets.push(format!("secret_version = ${idx}"));
            idx += 1;
        }
        if let Some(enabled) = patch.interlink_enabled {
            values.push(Box::new(enabled));
            sets.push(format!("interlink_enabled = ${idx}"));
            idx += 1;
        }
        if let Some(capabilities) = &patch.capabilities {
            push_str!("capabilities", capabilities);
        }
        if let Some(overrides) = &patch.policy_overrides {
            push_str!("policy_overrides", overrides);
        }
        if let Some(connected) = patch.tunnel_connected {
            values.push(Box::new(connected));
            sets.push(format!("tunnel_connected = ${idx}"));
            idx += 1;
        }
        if let Some(last_tunnel_at) = patch.last_tunnel_at {
            values.push(Box::new(last_tunnel_at));
            sets.push(format!("last_tunnel_at = ${idx}"));
            idx += 1;
        }
        if let Some(rotated_at) = patch.secret_rotated_at {
            values.push(Box::new(rotated_at));
            sets.push(format!("secret_rotated_at = ${idx}"));
            idx += 1;
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE cloud_devices SET {} WHERE device_id = ${idx}",
            sets.join(", ")
        );
        values.push(Box::new(cleaned.to_string()));
        let mut conn = self.conn()?;
        let refs: Vec<&(dyn ToSql + Sync)> =
            values.iter().map(|value| value.as_ref()).collect();
        conn.execute(&sql, &refs)?;
        Ok(())
    }

    fn upsert_interlink_channel_impl(&self, record: &InterlinkChannelRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.channel_id.trim().is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO interlink_channels(channel_id, device_id, user_id, client, instance_id, protocol_version, caps, connected_at, last_seen_at, rtt_ms, resumed_count, closed_reason) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) \
             ON CONFLICT(channel_id) DO UPDATE SET \
               last_seen_at = EXCLUDED.last_seen_at, rtt_ms = EXCLUDED.rtt_ms, \
               resumed_count = EXCLUDED.resumed_count, closed_reason = EXCLUDED.closed_reason",
            &[
                &record.channel_id.trim(),
                &record.device_id.trim(),
                &record.user_id.trim(),
                &record.client.trim(),
                &record.instance_id.trim(),
                &record.protocol_version,
                &record.caps,
                &record.connected_at,
                &record.last_seen_at,
                &record.rtt_ms,
                &record.resumed_count,
                &record.closed_reason,
            ],
        )?;
        Ok(())
    }

    fn get_interlink_channel_impl(
        &self,
        channel_id: &str,
    ) -> Result<Option<InterlinkChannelRecord>> {
        self.ensure_initialized()?;
        let cleaned = channel_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            &format!("SELECT {CHANNEL_COLUMNS} FROM interlink_channels WHERE channel_id = $1"),
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_channel_row(&row)))
    }

    fn close_interlink_channel_impl(&self, channel_id: &str, closed_reason: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = channel_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE interlink_channels SET closed_reason = $1 WHERE channel_id = $2 AND closed_reason IS NULL",
            &[&closed_reason.trim(), &cleaned],
        )?;
        Ok(())
    }

    fn list_interlink_channels_impl(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<InterlinkChannelRecord>, i64)> {
        self.ensure_initialized()?;
        let mut where_clause = String::new();
        let mut values: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(user_id) = user_id {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                values.push(Box::new(cleaned.to_string()));
                where_clause = " WHERE user_id = $1".to_string();
            }
        }
        let mut conn = self.conn()?;
        let refs: Vec<&(dyn ToSql + Sync)> =
            values.iter().map(|value| value.as_ref()).collect();
        let total: i64 = conn
            .query_one(
                &format!("SELECT COUNT(*) FROM interlink_channels{where_clause}"),
                &refs,
            )?
            .get(0);
        let mut rows = Vec::new();
        if limit > 0 {
            values.push(Box::new(limit));
            values.push(Box::new(offset.max(0)));
            let refs: Vec<&(dyn ToSql + Sync)> =
                values.iter().map(|value| value.as_ref()).collect();
            let idx = values.len();
            let fetched = conn.query(
                &format!(
                    "SELECT {CHANNEL_COLUMNS} FROM interlink_channels{where_clause} \
                     ORDER BY connected_at DESC LIMIT ${} OFFSET ${}",
                    idx - 1,
                    idx
                ),
                &refs,
            )?;
            rows = fetched.iter().map(map_channel_row).collect();
        }
        Ok((rows, total))
    }

    fn upsert_interlink_shadow_impl(&self, record: &InterlinkShadowRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.device_id.trim().is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO interlink_node_shadows(device_id, user_id, revision, summary, threads, tasks, workspace, synced_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8) \
             ON CONFLICT(device_id) DO UPDATE SET \
               user_id = EXCLUDED.user_id, revision = EXCLUDED.revision, \
               summary = EXCLUDED.summary, threads = EXCLUDED.threads, tasks = EXCLUDED.tasks, \
               workspace = EXCLUDED.workspace, synced_at = EXCLUDED.synced_at",
            &[
                &record.device_id.trim(),
                &record.user_id.trim(),
                &record.revision,
                &record.summary,
                &record.threads,
                &record.tasks,
                &record.workspace,
                &record.synced_at,
            ],
        )?;
        Ok(())
    }

    fn get_interlink_shadow_impl(
        &self,
        device_id: &str,
    ) -> Result<Option<InterlinkShadowRecord>> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT device_id, user_id, revision, summary, threads, tasks, workspace, synced_at \
             FROM interlink_node_shadows WHERE device_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_shadow_row(&row)))
    }

    fn get_interlink_shadow_revision_impl(&self, device_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let revision: i64 = conn
            .query_one(
                "SELECT COALESCE((SELECT revision FROM interlink_node_shadows WHERE device_id = $1), 0)::BIGINT",
                &[&cleaned],
            )?
            .get(0);
        Ok(revision)
    }

    fn delete_interlink_shadow_impl(&self, device_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "DELETE FROM interlink_node_shadows WHERE device_id = $1",
            &[&cleaned],
        )?;
        Ok(())
    }

    fn cleanup_interlink_shadows_impl(&self, retention_days: u32, max_rows: i64) -> Result<u64> {
        self.ensure_initialized()?;
        if retention_days == 0 || max_rows <= 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let budget = max_rows.min(SHADOW_CLEANUP_MAX_ROWS);
        let mut conn = self.conn()?;
        let mut removed = 0u64;
        let mut remaining = budget;
        while remaining > 0 {
            let page = remaining.min(SHADOW_CLEANUP_PAGE);
            let deleted = conn.execute(
                "DELETE FROM interlink_node_shadows WHERE device_id IN (\
                   SELECT device_id FROM interlink_node_shadows \
                   WHERE COALESCE(synced_at, 0) < $1 ORDER BY synced_at LIMIT $2\
                 )",
                &[&cutoff, &page],
            )?;
            if deleted == 0 {
                break;
            }
            removed += deleted;
            remaining -= deleted as i64;
        }
        Ok(removed)
    }

    fn insert_interlink_command_impl(&self, record: &InterlinkCommandRecord) -> Result<bool> {
        self.ensure_initialized()?;
        if record.command_id.trim().is_empty() {
            return Ok(false);
        }
        let mut conn = self.conn()?;
        let inserted = conn.execute(
            "INSERT INTO interlink_commands(command_id, direction, actor_user_id, from_node, to_node, kind, args_digest, approval_state, status, created_at, acked_at, finished_at, error_code, error_summary) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(command_id) DO NOTHING",
            &[
                &record.command_id.trim(),
                &record.direction.trim(),
                &record.actor_user_id.trim(),
                &record.from_node.trim(),
                &record.to_node.trim(),
                &record.kind.trim(),
                &record.args_digest,
                &record.approval_state.trim(),
                &record.status.trim(),
                &record.created_at,
                &record.acked_at,
                &record.finished_at,
                &record.error_code,
                &record.error_summary,
            ],
        )?;
        Ok(inserted > 0)
    }

    fn update_interlink_command_status_impl(
        &self,
        command_id: &str,
        status: &str,
        acked_at: Option<f64>,
        finished_at: Option<f64>,
        error_code: Option<&str>,
        error_summary: Option<&str>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = command_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        // First terminal state wins (mirrors the sqlite implementation).
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE interlink_commands \
             SET status = CASE WHEN finished_at IS NULL THEN $1 ELSE status END, \
                 acked_at = COALESCE(acked_at, $2), \
                 finished_at = COALESCE(finished_at, $3), \
                 error_code = CASE WHEN finished_at IS NULL THEN $4 ELSE error_code END, \
                 error_summary = CASE WHEN finished_at IS NULL THEN $5 ELSE error_summary END \
             WHERE command_id = $6",
            &[
                &status.trim(),
                &acked_at,
                &finished_at,
                &error_code,
                &error_summary,
                &cleaned,
            ],
        )?;
        Ok(())
    }

    fn set_interlink_command_approval_impl(
        &self,
        command_id: &str,
        approval_state: &str,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = command_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE interlink_commands SET approval_state = $1 WHERE command_id = $2",
            &[&approval_state.trim(), &cleaned],
        )?;
        Ok(())
    }

    fn get_interlink_command_impl(
        &self,
        command_id: &str,
    ) -> Result<Option<InterlinkCommandRecord>> {
        self.ensure_initialized()?;
        let cleaned = command_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            &format!("SELECT {COMMAND_COLUMNS} FROM interlink_commands WHERE command_id = $1"),
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_command_row(&row)))
    }

    fn list_interlink_commands_impl(
        &self,
        query: ListInterlinkCommandsQuery<'_>,
    ) -> Result<(Vec<InterlinkCommandRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut values: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        for (column, value) in [
            ("actor_user_id", query.user_id),
            ("to_node", query.device_id),
            ("kind", query.kind),
            ("status", query.status),
            ("direction", query.direction),
        ] {
            if let Some(value) = value {
                let cleaned = value.trim();
                if !cleaned.is_empty() {
                    values.push(Box::new(cleaned.to_string()));
                    conditions.push(format!("{column} = ${}", values.len()));
                }
            }
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let mut conn = self.conn()?;
        let refs: Vec<&(dyn ToSql + Sync)> =
            values.iter().map(|value| value.as_ref()).collect();
        let total: i64 = conn
            .query_one(
                &format!("SELECT COUNT(*) FROM interlink_commands{where_clause}"),
                &refs,
            )?
            .get(0);
        let mut rows = Vec::new();
        if query.limit > 0 {
            values.push(Box::new(query.limit));
            values.push(Box::new(query.offset.max(0)));
            let refs: Vec<&(dyn ToSql + Sync)> =
                values.iter().map(|value| value.as_ref()).collect();
            let idx = values.len();
            let fetched = conn.query(
                &format!(
                    "SELECT {COMMAND_COLUMNS} FROM interlink_commands{where_clause} \
                     ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
                    idx - 1,
                    idx
                ),
                &refs,
            )?;
            rows = fetched.iter().map(map_command_row).collect();
        }
        Ok((rows, total))
    }

    fn cleanup_interlink_commands_impl(&self, retention_days: u32) -> Result<u64> {
        self.ensure_initialized()?;
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let mut conn = self.conn()?;
        let mut removed = 0u64;
        let mut remaining = LEDGER_CLEANUP_MAX_ROWS;
        while remaining > 0 {
            let page = remaining.min(LEDGER_CLEANUP_PAGE);
            let deleted = conn.execute(
                "DELETE FROM interlink_commands WHERE command_id IN (\
                   SELECT command_id FROM interlink_commands \
                   WHERE COALESCE(created_at, 0) < $1 ORDER BY created_at LIMIT $2\
                 )",
                &[&cutoff, &page],
            )?;
            if deleted == 0 {
                break;
            }
            removed += deleted;
            remaining -= deleted as i64;
        }
        Ok(removed)
    }

    fn insert_interlink_approval_impl(&self, record: &InterlinkApprovalRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.approval_id.trim().is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO interlink_approvals(approval_id, command_id, device_id, user_id, prompt, risk_level, state, decided_by, decided_at, expires_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT(approval_id) DO NOTHING",
            &[
                &record.approval_id.trim(),
                &record.command_id.trim(),
                &record.device_id.trim(),
                &record.user_id.trim(),
                &record.prompt,
                &record.risk_level.trim(),
                &record.state.trim(),
                &record.decided_by,
                &record.decided_at,
                &record.expires_at,
            ],
        )?;
        Ok(())
    }

    fn decide_interlink_approval_impl(
        &self,
        approval_id: &str,
        state: &str,
        decided_by: &str,
        decided_at: f64,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = approval_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE interlink_approvals SET state = $1, decided_by = $2, decided_at = $3 \
             WHERE approval_id = $4 AND state = 'pending'",
            &[&state.trim(), &decided_by.trim(), &decided_at, &cleaned],
        )?;
        Ok(())
    }

    fn get_interlink_approval_impl(
        &self,
        approval_id: &str,
    ) -> Result<Option<InterlinkApprovalRecord>> {
        self.ensure_initialized()?;
        let cleaned = approval_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT approval_id, command_id, device_id, user_id, prompt, risk_level, state, decided_by, decided_at, expires_at \
             FROM interlink_approvals WHERE approval_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_approval_row(&row)))
    }

    fn insert_interlink_audit_impl(&self, record: &InterlinkAuditRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO interlink_audit(command_id, approval_id, actor, from_node, to_node, action, detail_digest, created_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            &[
                &record.command_id,
                &record.approval_id,
                &record.actor.trim(),
                &record.from_node,
                &record.to_node,
                &record.action.trim(),
                &record.detail_digest,
                &record.created_at,
            ],
        )?;
        Ok(())
    }

    fn list_interlink_audit_impl(
        &self,
        query: ListInterlinkAuditQuery<'_>,
    ) -> Result<(Vec<InterlinkAuditRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut values: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        for (column, value) in [("actor", query.user_id), ("action", query.action)] {
            if let Some(value) = value {
                let cleaned = value.trim();
                if !cleaned.is_empty() {
                    values.push(Box::new(cleaned.to_string()));
                    conditions.push(format!("{column} = ${}", values.len()));
                }
            }
        }
        if let Some(device_id) = query.device_id {
            let cleaned = device_id.trim();
            if !cleaned.is_empty() {
                values.push(Box::new(cleaned.to_string()));
                conditions.push(format!("(from_node = ${0} OR to_node = ${0})", values.len()));
            }
        }
        if let Some(since) = query.since {
            values.push(Box::new(since));
            conditions.push(format!("created_at >= ${}", values.len()));
        }
        if let Some(until) = query.until {
            values.push(Box::new(until));
            conditions.push(format!("created_at <= ${}", values.len()));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let mut conn = self.conn()?;
        let refs: Vec<&(dyn ToSql + Sync)> =
            values.iter().map(|value| value.as_ref()).collect();
        let total: i64 = conn
            .query_one(
                &format!("SELECT COUNT(*) FROM interlink_audit{where_clause}"),
                &refs,
            )?
            .get(0);
        let mut rows = Vec::new();
        if query.limit > 0 {
            values.push(Box::new(query.limit));
            values.push(Box::new(query.offset.max(0)));
            let refs: Vec<&(dyn ToSql + Sync)> =
                values.iter().map(|value| value.as_ref()).collect();
            let idx = values.len();
            let fetched = conn.query(
                &format!(
                    "SELECT seq, command_id, approval_id, actor, from_node, to_node, action, detail_digest, created_at \
                     FROM interlink_audit{where_clause} ORDER BY seq DESC LIMIT ${} OFFSET ${}",
                    idx - 1,
                    idx
                ),
                &refs,
            )?;
            rows = fetched
                .iter()
                .map(|row| InterlinkAuditRecord {
                    seq: row.get(0),
                    command_id: row.get(1),
                    approval_id: row.get(2),
                    actor: row.get::<_, Option<String>>(3).unwrap_or_default(),
                    from_node: row.get(4),
                    to_node: row.get(5),
                    action: row.get::<_, Option<String>>(6).unwrap_or_default(),
                    detail_digest: row.get(7),
                    created_at: row.get::<_, Option<f64>>(8).unwrap_or(0.0),
                })
                .collect();
        }
        Ok((rows, total))
    }

    fn cleanup_interlink_audit_impl(&self, retention_days: u32) -> Result<u64> {
        self.ensure_initialized()?;
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let mut conn = self.conn()?;
        let mut removed = 0u64;
        let mut remaining = LEDGER_CLEANUP_MAX_ROWS;
        while remaining > 0 {
            let page = remaining.min(LEDGER_CLEANUP_PAGE);
            let deleted = conn.execute(
                "DELETE FROM interlink_audit WHERE seq IN (\
                   SELECT seq FROM interlink_audit \
                   WHERE COALESCE(created_at, 0) < $1 ORDER BY created_at LIMIT $2\
                 )",
                &[&cutoff, &page],
            )?;
            if deleted == 0 {
                break;
            }
            removed += deleted;
            remaining -= deleted as i64;
        }
        Ok(removed)
    }
}
