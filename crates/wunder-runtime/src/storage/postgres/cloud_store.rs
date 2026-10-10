use super::PostgresStorage;
use crate::storage::{
    CloudCallRecord, CloudDeviceInterlinkPatch, CloudDeviceLogRecord, CloudDeviceRecord,
    CloudLogInsertResult, ListCloudDeviceLogsQuery, ListCloudRecordsQuery, StorageLifecycle,
};
use anyhow::Result;
use tokio_postgres::types::ToSql;
use tokio_postgres::Row;

/// Column order shared by every `cloud_devices` reader: the base device fields
/// followed by the interlink extension in `CloudDeviceInterlinkPatch` order.
/// `map_cloud_device_row` maps these positions, so the two must stay in sync.
const CLOUD_DEVICE_COLUMNS: &str = "device_id, user_id, client, name, os, arch, app_version, \
    last_seen_at, created_at, revoked, node_secret_hash, secret_version, interlink_enabled, \
    capabilities, policy_overrides, tunnel_connected, last_tunnel_at, secret_rotated_at";

pub(super) trait PostgresCloudStorage {
    fn find_cloud_device_by_identity_impl(
        &self,
        user_id: &str,
        client: &str,
        name: &str,
    ) -> Result<Option<CloudDeviceRecord>>;
    fn upsert_cloud_device_impl(&self, record: &CloudDeviceRecord) -> Result<()>;
    fn get_cloud_device_impl(&self, device_id: &str) -> Result<Option<CloudDeviceRecord>>;
    fn touch_cloud_device_impl(&self, device_id: &str, last_seen_at: f64) -> Result<()>;
    fn set_cloud_device_revoked_impl(&self, device_id: &str, revoked: bool) -> Result<()>;
    fn list_cloud_devices_impl(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<CloudDeviceRecord>, i64)>;
    fn insert_cloud_call_record_impl(&self, record: &CloudCallRecord) -> Result<()>;
    fn finalize_cloud_call_record_impl(
        &self,
        call_id: &str,
        status: &str,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        finished_at: f64,
        error_summary: Option<&str>,
    ) -> Result<()>;
    fn list_cloud_call_records_impl(
        &self,
        query: ListCloudRecordsQuery<'_>,
    ) -> Result<(Vec<CloudCallRecord>, i64)>;
    fn insert_cloud_device_logs_impl(
        &self,
        records: &[CloudDeviceLogRecord],
    ) -> Result<CloudLogInsertResult>;
    fn latest_cloud_device_log_seq_impl(&self, device_id: &str) -> Result<i64>;
    fn list_cloud_device_logs_impl(
        &self,
        query: ListCloudDeviceLogsQuery<'_>,
    ) -> Result<(Vec<CloudDeviceLogRecord>, i64)>;
    fn cleanup_cloud_device_logs_impl(&self, retention_days: u32) -> Result<u64>;
}

fn map_cloud_device_row(row: &Row) -> CloudDeviceRecord {
    // The interlink extension is read with the base row (`CLOUD_DEVICE_COLUMNS`):
    // both backends expose the same patch, only flags are BOOLEAN here.
    let interlink = CloudDeviceInterlinkPatch {
        node_secret_hash: row.get(10),
        secret_version: row.get::<_, Option<i64>>(11).unwrap_or(0),
        interlink_enabled: row.get::<_, Option<bool>>(12),
        capabilities: row.get(13),
        policy_overrides: row.get(14),
        tunnel_connected: row.get::<_, Option<bool>>(15),
        last_tunnel_at: row.get(16),
        secret_rotated_at: row.get(17),
    };
    CloudDeviceRecord {
        device_id: row.get(0),
        user_id: row.get(1),
        client: row.get(2),
        name: row.get(3),
        os: row.get(4),
        arch: row.get(5),
        app_version: row.get(6),
        last_seen_at: row.get::<_, Option<f64>>(7).unwrap_or(0.0),
        created_at: row.get::<_, Option<f64>>(8).unwrap_or(0.0),
        revoked: row.get::<_, bool>(9),
        interlink: Some(Box::new(interlink)),
    }
}

fn map_cloud_device_log_row(row: &Row) -> CloudDeviceLogRecord {
    CloudDeviceLogRecord {
        device_id: row.get(0),
        seq: row.get(1),
        user_id: row.get::<_, Option<String>>(2).unwrap_or_default(),
        client: row.get::<_, Option<String>>(3).unwrap_or_default(),
        level: row.get::<_, Option<String>>(4).unwrap_or_default(),
        category: row.get::<_, Option<String>>(5).unwrap_or_default(),
        event: row.get::<_, Option<String>>(6).unwrap_or_default(),
        message: row.get(7),
        local_session_id: row.get(8),
        created_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
    }
}

impl PostgresCloudStorage for PostgresStorage {
    fn find_cloud_device_by_identity_impl(
        &self,
        user_id: &str,
        client: &str,
        name: &str,
    ) -> Result<Option<CloudDeviceRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_client = client.trim();
        let cleaned_name = name.trim();
        if cleaned_user.is_empty() || cleaned_client.is_empty() || cleaned_name.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let sql = format!(
            "SELECT {CLOUD_DEVICE_COLUMNS} FROM cloud_devices \
             WHERE user_id = $1 AND client = $2 AND name = $3"
        );
        let row = conn.query_opt(&sql, &[&cleaned_user, &cleaned_client, &cleaned_name])?;
        Ok(row.map(|row| map_cloud_device_row(&row)))
    }

    fn upsert_cloud_device_impl(&self, record: &CloudDeviceRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.device_id.trim().is_empty() {
            return Ok(());
        }
        // Re-login refreshes diagnostics and last_seen_at; the stored revoked
        // flag and created_at are never reset by an upsert.
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO cloud_devices(device_id, user_id, client, name, os, arch, app_version, last_seen_at, created_at, revoked) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
             ON CONFLICT(device_id) DO UPDATE SET \
               user_id = EXCLUDED.user_id, client = EXCLUDED.client, name = EXCLUDED.name, \
               os = EXCLUDED.os, arch = EXCLUDED.arch, app_version = EXCLUDED.app_version, \
               last_seen_at = EXCLUDED.last_seen_at",
            &[
                &record.device_id,
                &record.user_id,
                &record.client,
                &record.name,
                &record.os,
                &record.arch,
                &record.app_version,
                &record.last_seen_at,
                &record.created_at,
                &record.revoked,
            ],
        )?;
        Ok(())
    }

    fn get_cloud_device_impl(&self, device_id: &str) -> Result<Option<CloudDeviceRecord>> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let mut conn = self.conn()?;
        let sql = format!("SELECT {CLOUD_DEVICE_COLUMNS} FROM cloud_devices WHERE device_id = $1");
        let row = conn.query_opt(&sql, &[&cleaned])?;
        Ok(row.map(|row| map_cloud_device_row(&row)))
    }

    fn touch_cloud_device_impl(&self, device_id: &str, last_seen_at: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE cloud_devices SET last_seen_at = $1 WHERE device_id = $2",
            &[&last_seen_at, &cleaned],
        )?;
        Ok(())
    }

    fn set_cloud_device_revoked_impl(&self, device_id: &str, revoked: bool) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE cloud_devices SET revoked = $1 WHERE device_id = $2",
            &[&revoked, &cleaned],
        )?;
        Ok(())
    }

    fn list_cloud_devices_impl(
        &self,
        user_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<CloudDeviceRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(user_id) = user_id.map(str::trim).filter(|value| !value.is_empty()) {
            params.push(Box::new(user_id.to_string()));
            filters.push(format!("user_id = ${}", params.len()));
        }
        let mut count_sql = "SELECT COUNT(*) FROM cloud_devices".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let total: i64 = conn.query_one(&count_sql, &count_refs)?.get(0);
        let mut rows = Vec::new();
        if limit > 0 {
            let mut sql = format!("SELECT {CLOUD_DEVICE_COLUMNS} FROM cloud_devices");
            if !filters.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&filters.join(" AND "));
            }
            sql.push_str(" ORDER BY last_seen_at DESC LIMIT $");
            params.push(Box::new(limit));
            sql.push_str(&params.len().to_string());
            sql.push_str(" OFFSET $");
            params.push(Box::new(offset.max(0)));
            sql.push_str(&params.len().to_string());
            let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
            rows = conn.query(&sql, &refs)?;
        }
        let output = rows.iter().map(map_cloud_device_row).collect();
        Ok((output, total))
    }

    fn insert_cloud_call_record_impl(&self, record: &CloudCallRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        conn.execute(
            "INSERT INTO cloud_call_records(call_id, device_id, user_id, client, local_session_id, model, provider, status, quota_consumed, queue_waited_ms, prompt_tokens, completion_tokens, started_at, finished_at, duration_ms, error_summary) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
            &[
                &record.call_id,
                &record.device_id,
                &record.user_id,
                &record.client,
                &record.local_session_id,
                &record.model,
                &record.provider,
                &record.status,
                &record.quota_consumed,
                &record.queue_waited_ms,
                &record.prompt_tokens,
                &record.completion_tokens,
                &record.started_at,
                &record.finished_at,
                &record.duration_ms,
                &record.error_summary,
            ],
        )?;
        Ok(())
    }

    fn finalize_cloud_call_record_impl(
        &self,
        call_id: &str,
        status: &str,
        prompt_tokens: Option<i64>,
        completion_tokens: Option<i64>,
        finished_at: f64,
        error_summary: Option<&str>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = call_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        // duration_ms is derived from the stored started_at so admission and
        // settlement cannot disagree about the call duration.
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE cloud_call_records \
             SET status = $1, prompt_tokens = $2, completion_tokens = $3, finished_at = $4, \
                 duration_ms = GREATEST(TRUNC(($4 - started_at) * 1000), 0)::BIGINT, error_summary = $5 \
             WHERE call_id = $6",
            &[
                &status.trim(),
                &prompt_tokens,
                &completion_tokens,
                &finished_at,
                &error_summary,
                &cleaned,
            ],
        )?;
        Ok(())
    }

    fn list_cloud_call_records_impl(
        &self,
        query: ListCloudRecordsQuery<'_>,
    ) -> Result<(Vec<CloudCallRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        for (column, value) in [
            ("user_id", query.user_id),
            ("device_id", query.device_id),
            ("model", query.model),
            ("status", query.status),
        ] {
            if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
                params.push(Box::new(value.to_string()));
                filters.push(format!("{column} = ${}", params.len()));
            }
        }
        let mut count_sql = "SELECT COUNT(*) FROM cloud_call_records".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let total: i64 = conn.query_one(&count_sql, &count_refs)?.get(0);
        let mut rows = Vec::new();
        if query.limit > 0 {
            let mut sql = "SELECT call_id, device_id, user_id, client, local_session_id, model, provider, status, quota_consumed, queue_waited_ms, prompt_tokens, completion_tokens, started_at, finished_at, duration_ms, error_summary FROM cloud_call_records".to_string();
            if !filters.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&filters.join(" AND "));
            }
            sql.push_str(" ORDER BY started_at DESC LIMIT $");
            params.push(Box::new(query.limit));
            sql.push_str(&params.len().to_string());
            sql.push_str(" OFFSET $");
            params.push(Box::new(query.offset.max(0)));
            sql.push_str(&params.len().to_string());
            let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
            rows = conn.query(&sql, &refs)?;
        }
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(CloudCallRecord {
                call_id: row.get(0),
                device_id: row.get::<_, Option<String>>(1).unwrap_or_default(),
                user_id: row.get::<_, Option<String>>(2).unwrap_or_default(),
                client: row.get::<_, Option<String>>(3).unwrap_or_default(),
                local_session_id: row.get(4),
                model: row.get::<_, Option<String>>(5).unwrap_or_default(),
                provider: row.get(6),
                status: row.get::<_, Option<String>>(7).unwrap_or_default(),
                quota_consumed: row.get::<_, bool>(8),
                queue_waited_ms: row.get::<_, Option<i64>>(9).unwrap_or(0),
                prompt_tokens: row.get(10),
                completion_tokens: row.get(11),
                started_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                finished_at: row.get(13),
                duration_ms: row.get(14),
                error_summary: row.get(15),
            });
        }
        Ok((output, total))
    }

    fn insert_cloud_device_logs_impl(
        &self,
        records: &[CloudDeviceLogRecord],
    ) -> Result<CloudLogInsertResult> {
        self.ensure_initialized()?;
        if records.is_empty() {
            return Ok(CloudLogInsertResult {
                accepted: 0,
                last_seq: None,
            });
        }
        // last_seq reports the highest seq submitted in this request (not just
        // newly accepted rows) so the reporter always advances to its cursor.
        let last_seq = records.iter().map(|record| record.seq).max();
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let mut accepted = 0_i64;
        for record in records {
            let inserted = tx.execute(
                "INSERT INTO cloud_device_logs(device_id, seq, user_id, client, level, category, event, message, local_session_id, created_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                 ON CONFLICT (device_id, seq) DO NOTHING",
                &[
                    &record.device_id,
                    &record.seq,
                    &record.user_id,
                    &record.client,
                    &record.level,
                    &record.category,
                    &record.event,
                    &record.message,
                    &record.local_session_id,
                    &record.created_at,
                ],
            )?;
            accepted += inserted as i64;
        }
        tx.commit()?;
        Ok(CloudLogInsertResult { accepted, last_seq })
    }

    fn latest_cloud_device_log_seq_impl(&self, device_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT COALESCE(MAX(seq), 0) FROM cloud_device_logs WHERE device_id = $1",
            &[&cleaned],
        )?;
        Ok(row.get::<_, i64>(0))
    }

    fn list_cloud_device_logs_impl(
        &self,
        query: ListCloudDeviceLogsQuery<'_>,
    ) -> Result<(Vec<CloudDeviceLogRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        for (column, value) in [
            ("user_id", query.user_id),
            ("device_id", query.device_id),
            ("level", query.level),
            ("category", query.category),
        ] {
            if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
                params.push(Box::new(value.to_string()));
                filters.push(format!("{column} = ${}", params.len()));
            }
        }
        let mut count_sql = "SELECT COUNT(*) FROM cloud_device_logs".to_string();
        if !filters.is_empty() {
            count_sql.push_str(" WHERE ");
            count_sql.push_str(&filters.join(" AND "));
        }
        let count_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let total: i64 = conn.query_one(&count_sql, &count_refs)?.get(0);
        let mut rows = Vec::new();
        if query.limit > 0 {
            // The (device_id, seq) primary key serves this ordering directly.
            let mut sql = "SELECT device_id, seq, user_id, client, level, category, event, message, local_session_id, created_at FROM cloud_device_logs".to_string();
            if !filters.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&filters.join(" AND "));
            }
            sql.push_str(" ORDER BY device_id DESC, seq DESC LIMIT $");
            params.push(Box::new(query.limit));
            sql.push_str(&params.len().to_string());
            sql.push_str(" OFFSET $");
            params.push(Box::new(query.offset.max(0)));
            sql.push_str(&params.len().to_string());
            let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
            rows = conn.query(&sql, &refs)?;
        }
        let output = rows.iter().map(map_cloud_device_log_row).collect();
        Ok((output, total))
    }

    fn cleanup_cloud_device_logs_impl(&self, retention_days: u32) -> Result<u64> {
        self.ensure_initialized()?;
        // Zero retention means "never expire": log cleanup is disabled.
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let mut conn = self.conn()?;
        let deleted = conn.execute(
            "DELETE FROM cloud_device_logs WHERE created_at < $1",
            &[&cutoff],
        )?;
        Ok(deleted as u64)
    }
}
