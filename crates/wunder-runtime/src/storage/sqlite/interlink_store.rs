use super::SqliteStorage;
use crate::storage::{
    CloudDeviceInterlinkPatch, InterlinkApprovalRecord, InterlinkAuditRecord,
    InterlinkChannelRecord, InterlinkCommandRecord, InterlinkShadowRecord,
    ListInterlinkAuditQuery, ListInterlinkCommandsQuery, StorageLifecycle,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};

pub(super) trait SqliteInterlinkStorage {
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

impl SqliteInterlinkStorage for SqliteStorage {
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
        let mut values: Vec<SqlValue> = Vec::new();
        if let Some(secret_hash) = &patch.node_secret_hash {
            sets.push("node_secret_hash = ?");
            values.push(SqlValue::from(secret_hash.clone()));
        }
        if patch.secret_version > 0 {
            sets.push("secret_version = ?");
            values.push(SqlValue::from(patch.secret_version));
        }
        if let Some(enabled) = patch.interlink_enabled {
            sets.push("interlink_enabled = ?");
            values.push(SqlValue::from(enabled as i64));
        }
        if let Some(capabilities) = &patch.capabilities {
            sets.push("capabilities = ?");
            values.push(SqlValue::from(capabilities.clone()));
        }
        if let Some(overrides) = &patch.policy_overrides {
            sets.push("policy_overrides = ?");
            values.push(SqlValue::from(overrides.clone()));
        }
        if let Some(connected) = patch.tunnel_connected {
            sets.push("tunnel_connected = ?");
            values.push(SqlValue::from(connected as i64));
        }
        if let Some(last_tunnel_at) = patch.last_tunnel_at {
            sets.push("last_tunnel_at = ?");
            values.push(SqlValue::from(last_tunnel_at));
        }
        if let Some(rotated_at) = patch.secret_rotated_at {
            sets.push("secret_rotated_at = ?");
            values.push(SqlValue::from(rotated_at));
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE cloud_devices SET {} WHERE device_id = ?",
            sets.join(", ")
        );
        values.push(SqlValue::from(cleaned.to_string()));
        self.open()?.execute(&sql, params_from_iter(values.iter()))?;
        Ok(())
    }

    fn upsert_interlink_channel_impl(&self, record: &InterlinkChannelRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.channel_id.trim().is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "INSERT INTO interlink_channels(channel_id, device_id, user_id, client, instance_id, protocol_version, caps, connected_at, last_seen_at, rtt_ms, resumed_count, closed_reason) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(channel_id) DO UPDATE SET \
               last_seen_at=excluded.last_seen_at, rtt_ms=excluded.rtt_ms, \
               resumed_count=excluded.resumed_count, closed_reason=excluded.closed_reason",
            params![
                record.channel_id.trim(),
                record.device_id.trim(),
                record.user_id.trim(),
                record.client.trim(),
                record.instance_id.trim(),
                record.protocol_version,
                record.caps,
                record.connected_at,
                record.last_seen_at,
                record.rtt_ms,
                record.resumed_count,
                record.closed_reason,
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel_id, device_id, user_id, client, instance_id, protocol_version, caps, connected_at, last_seen_at, rtt_ms, resumed_count, closed_reason \
                 FROM interlink_channels WHERE channel_id = ?",
                params![cleaned],
                |row| Self::read_interlink_channel(row),
            )
            .optional()?;
        Ok(row)
    }

    fn close_interlink_channel_impl(&self, channel_id: &str, closed_reason: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = channel_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "UPDATE interlink_channels SET closed_reason = ? WHERE channel_id = ? AND closed_reason IS NULL",
            params![closed_reason.trim(), cleaned],
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
        let mut conditions = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        if let Some(user_id) = user_id {
            let cleaned = user_id.trim();
            if !cleaned.is_empty() {
                conditions.push("user_id = ?".to_string());
                params_list.push(SqlValue::from(cleaned.to_string()));
            }
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM interlink_channels{where_clause}"),
            params_from_iter(params_list.iter()),
            |row| row.get(0),
        )?;
        let mut rows = Vec::new();
        if limit > 0 {
            let mut sql = format!(
                "SELECT channel_id, device_id, user_id, client, instance_id, protocol_version, caps, connected_at, last_seen_at, rtt_ms, resumed_count, closed_reason \
                 FROM interlink_channels{where_clause} ORDER BY connected_at DESC"
            );
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
            let mut stmt = conn.prepare(&sql)?;
            rows = stmt
                .query_map(params_from_iter(params_list.iter()), |row| {
                    Self::read_interlink_channel(row)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        Ok((rows, total))
    }

    fn upsert_interlink_shadow_impl(&self, record: &InterlinkShadowRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.device_id.trim().is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "INSERT INTO interlink_node_shadows(device_id, user_id, revision, summary, threads, tasks, workspace, synced_at) \
             VALUES(?,?,?,?,?,?,?,?) \
             ON CONFLICT(device_id) DO UPDATE SET \
               user_id=excluded.user_id, revision=excluded.revision, \
               summary=excluded.summary, threads=excluded.threads, tasks=excluded.tasks, \
               workspace=excluded.workspace, synced_at=excluded.synced_at",
            params![
                record.device_id.trim(),
                record.user_id.trim(),
                record.revision,
                record.summary,
                record.threads,
                record.tasks,
                record.workspace,
                record.synced_at,
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT device_id, user_id, revision, summary, threads, tasks, workspace, synced_at \
                 FROM interlink_node_shadows WHERE device_id = ?",
                params![cleaned],
                |row| Self::read_interlink_shadow(row),
            )
            .optional()?;
        Ok(row)
    }

    fn get_interlink_shadow_revision_impl(&self, device_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let revision: i64 = conn.query_row(
            "SELECT COALESCE((SELECT revision FROM interlink_node_shadows WHERE device_id = ?), 0)",
            params![cleaned],
            |row| row.get(0),
        )?;
        Ok(revision)
    }

    fn delete_interlink_shadow_impl(&self, device_id: &str) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "DELETE FROM interlink_node_shadows WHERE device_id = ?",
            params![cleaned],
        )?;
        Ok(())
    }

    fn insert_interlink_command_impl(&self, record: &InterlinkCommandRecord) -> Result<bool> {
        self.ensure_initialized()?;
        if record.command_id.trim().is_empty() {
            return Ok(false);
        }
        // Idempotent insert: re-submitted command ids are ignored so network
        // retries can never double-execute.
        let inserted = self.open()?.execute(
            "INSERT OR IGNORE INTO interlink_commands(command_id, direction, actor_user_id, from_node, to_node, kind, args_digest, approval_state, status, created_at, acked_at, finished_at, error_code, error_summary) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                record.command_id.trim(),
                record.direction.trim(),
                record.actor_user_id.trim(),
                record.from_node.trim(),
                record.to_node.trim(),
                record.kind.trim(),
                record.args_digest,
                record.approval_state.trim(),
                record.status.trim(),
                record.created_at,
                record.acked_at,
                record.finished_at,
                record.error_code,
                record.error_summary,
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
        // First terminal state wins: once finished_at is set, later updates
        // only fill missing timestamps, never overwrite the terminal fields.
        self.open()?.execute(
            "UPDATE interlink_commands \
             SET status = CASE WHEN finished_at IS NULL THEN ? ELSE status END, \
                 acked_at = COALESCE(acked_at, ?), \
                 finished_at = COALESCE(finished_at, ?), \
                 error_code = CASE WHEN finished_at IS NULL THEN ? ELSE error_code END, \
                 error_summary = CASE WHEN finished_at IS NULL THEN ? ELSE error_summary END \
             WHERE command_id = ?",
            params![
                status.trim(),
                acked_at,
                finished_at,
                error_code,
                error_summary,
                cleaned,
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
        self.open()?.execute(
            "UPDATE interlink_commands SET approval_state = ? WHERE command_id = ?",
            params![approval_state.trim(), cleaned],
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT command_id, direction, actor_user_id, from_node, to_node, kind, args_digest, approval_state, status, created_at, acked_at, finished_at, error_code, error_summary \
                 FROM interlink_commands WHERE command_id = ?",
                params![cleaned],
                |row| Self::read_interlink_command(row),
            )
            .optional()?;
        Ok(row)
    }

    fn list_interlink_commands_impl(
        &self,
        query: ListInterlinkCommandsQuery<'_>,
    ) -> Result<(Vec<InterlinkCommandRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
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
                    conditions.push(format!("{column} = ?"));
                    params_list.push(SqlValue::from(cleaned.to_string()));
                }
            }
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM interlink_commands{where_clause}"),
            params_from_iter(params_list.iter()),
            |row| row.get(0),
        )?;
        let mut rows = Vec::new();
        if query.limit > 0 {
            let mut sql = format!(
                "SELECT command_id, direction, actor_user_id, from_node, to_node, kind, args_digest, approval_state, status, created_at, acked_at, finished_at, error_code, error_summary \
                 FROM interlink_commands{where_clause} ORDER BY created_at DESC"
            );
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(query.limit));
            params_list.push(SqlValue::from(query.offset.max(0)));
            let mut stmt = conn.prepare(&sql)?;
            rows = stmt
                .query_map(params_from_iter(params_list.iter()), |row| {
                    Self::read_interlink_command(row)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        Ok((rows, total))
    }

    fn cleanup_interlink_commands_impl(&self, retention_days: u32) -> Result<u64> {
        self.ensure_initialized()?;
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let deleted = self.open()?.execute(
            "DELETE FROM interlink_commands WHERE COALESCE(created_at, 0) < ?",
            params![cutoff],
        )?;
        Ok(deleted as u64)
    }

    fn insert_interlink_approval_impl(&self, record: &InterlinkApprovalRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.approval_id.trim().is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "INSERT OR IGNORE INTO interlink_approvals(approval_id, command_id, device_id, user_id, prompt, risk_level, state, decided_by, decided_at, expires_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?)",
            params![
                record.approval_id.trim(),
                record.command_id.trim(),
                record.device_id.trim(),
                record.user_id.trim(),
                record.prompt,
                record.risk_level.trim(),
                record.state.trim(),
                record.decided_by,
                record.decided_at,
                record.expires_at,
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
        // Pending-only decision: an already decided approval cannot be flipped.
        self.open()?.execute(
            "UPDATE interlink_approvals SET state = ?, decided_by = ?, decided_at = ? \
             WHERE approval_id = ? AND state = 'pending'",
            params![state.trim(), decided_by.trim(), decided_at, cleaned],
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT approval_id, command_id, device_id, user_id, prompt, risk_level, state, decided_by, decided_at, expires_at \
                 FROM interlink_approvals WHERE approval_id = ?",
                params![cleaned],
                |row| Self::read_interlink_approval(row),
            )
            .optional()?;
        Ok(row)
    }

    fn insert_interlink_audit_impl(&self, record: &InterlinkAuditRecord) -> Result<()> {
        self.ensure_initialized()?;
        self.open()?.execute(
            "INSERT INTO interlink_audit(command_id, approval_id, actor, from_node, to_node, action, detail_digest, created_at) \
             VALUES(?,?,?,?,?,?,?,?)",
            params![
                record.command_id,
                record.approval_id,
                record.actor.trim(),
                record.from_node,
                record.to_node,
                record.action.trim(),
                record.detail_digest,
                record.created_at,
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
        let mut params_list: Vec<SqlValue> = Vec::new();
        for (column, value) in [("actor", query.user_id), ("action", query.action)] {
            if let Some(value) = value {
                let cleaned = value.trim();
                if !cleaned.is_empty() {
                    conditions.push(format!("{column} = ?"));
                    params_list.push(SqlValue::from(cleaned.to_string()));
                }
            }
        }
        if let Some(device_id) = query.device_id {
            let cleaned = device_id.trim();
            if !cleaned.is_empty() {
                conditions.push("(from_node = ? OR to_node = ?)".to_string());
                params_list.push(SqlValue::from(cleaned.to_string()));
                params_list.push(SqlValue::from(cleaned.to_string()));
            }
        }
        if let Some(since) = query.since {
            conditions.push("created_at >= ?".to_string());
            params_list.push(SqlValue::from(since));
        }
        if let Some(until) = query.until {
            conditions.push("created_at <= ?".to_string());
            params_list.push(SqlValue::from(until));
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM interlink_audit{where_clause}"),
            params_from_iter(params_list.iter()),
            |row| row.get(0),
        )?;
        let mut rows = Vec::new();
        if query.limit > 0 {
            let mut sql = format!(
                "SELECT seq, command_id, approval_id, actor, from_node, to_node, action, detail_digest, created_at \
                 FROM interlink_audit{where_clause} ORDER BY seq DESC"
            );
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(query.limit));
            params_list.push(SqlValue::from(query.offset.max(0)));
            let mut stmt = conn.prepare(&sql)?;
            rows = stmt
                .query_map(params_from_iter(params_list.iter()), |row| {
                    Ok(InterlinkAuditRecord {
                        seq: row.get(0)?,
                        command_id: row.get(1)?,
                        approval_id: row.get(2)?,
                        actor: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        from_node: row.get(4)?,
                        to_node: row.get(5)?,
                        action: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                        detail_digest: row.get(7)?,
                        created_at: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        Ok((rows, total))
    }

    fn cleanup_interlink_audit_impl(&self, retention_days: u32) -> Result<u64> {
        self.ensure_initialized()?;
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let deleted = self.open()?.execute(
            "DELETE FROM interlink_audit WHERE COALESCE(created_at, 0) < ?",
            params![cutoff],
        )?;
        Ok(deleted as u64)
    }
}

impl SqliteStorage {
    fn read_interlink_channel(row: &rusqlite::Row<'_>) -> rusqlite::Result<InterlinkChannelRecord> {
        Ok(InterlinkChannelRecord {
            channel_id: row.get(0)?,
            device_id: row.get(1)?,
            user_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            client: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            instance_id: row.get::<_, Option<String>>(4)?.unwrap_or_else(|| "local".to_string()),
            protocol_version: row.get::<_, Option<i64>>(5)?.unwrap_or(1),
            caps: row.get(6)?,
            connected_at: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
            last_seen_at: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
            rtt_ms: row.get(9)?,
            resumed_count: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
            closed_reason: row.get(11)?,
        })
    }

    fn read_interlink_shadow(row: &rusqlite::Row<'_>) -> rusqlite::Result<InterlinkShadowRecord> {
        Ok(InterlinkShadowRecord {
            device_id: row.get(0)?,
            user_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            revision: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            summary: row.get(3)?,
            threads: row.get(4)?,
            tasks: row.get(5)?,
            workspace: row.get(6)?,
            synced_at: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
        })
    }

    fn read_interlink_command(row: &rusqlite::Row<'_>) -> rusqlite::Result<InterlinkCommandRecord> {
        Ok(InterlinkCommandRecord {
            command_id: row.get(0)?,
            direction: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            actor_user_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            from_node: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            to_node: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            kind: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            args_digest: row.get(6)?,
            approval_state: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "none".to_string()),
            status: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "issued".to_string()),
            created_at: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
            acked_at: row.get(10)?,
            finished_at: row.get(11)?,
            error_code: row.get(12)?,
            error_summary: row.get(13)?,
        })
    }

    fn read_interlink_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<InterlinkApprovalRecord> {
        Ok(InterlinkApprovalRecord {
            approval_id: row.get(0)?,
            command_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            device_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            user_id: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            prompt: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            risk_level: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            state: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "pending".to_string()),
            decided_by: row.get(7)?,
            decided_at: row.get(8)?,
            expires_at: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
        })
    }
}
