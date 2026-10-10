use super::SqliteStorage;
use crate::storage::{
    CloudCallRecord, CloudDeviceLogRecord, CloudDeviceRecord, CloudLogInsertResult,
    ListCloudDeviceLogsQuery, ListCloudRecordsQuery, StorageLifecycle,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension, TransactionBehavior};

pub(super) trait SqliteCloudStorage {
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

impl SqliteCloudStorage for SqliteStorage {
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT device_id, user_id, client, name, os, arch, app_version, last_seen_at, created_at, revoked \
                 FROM cloud_devices WHERE user_id = ? AND client = ? AND name = ?",
                params![cleaned_user, cleaned_client, cleaned_name],
                |row| Self::read_cloud_device(row),
            )
            .optional()?;
        Ok(row)
    }

    fn upsert_cloud_device_impl(&self, record: &CloudDeviceRecord) -> Result<()> {
        self.ensure_initialized()?;
        if record.device_id.trim().is_empty() {
            return Ok(());
        }
        // Re-login refreshes diagnostics and last_seen_at; the stored revoked
        // flag and created_at are never reset by an upsert.
        self.open()?.execute(
            "INSERT INTO cloud_devices(device_id, user_id, client, name, os, arch, app_version, last_seen_at, created_at, revoked) \
             VALUES(?,?,?,?,?,?,?,?,?,?) \
             ON CONFLICT(device_id) DO UPDATE SET \
               user_id=excluded.user_id, client=excluded.client, name=excluded.name, \
               os=excluded.os, arch=excluded.arch, app_version=excluded.app_version, \
               last_seen_at=excluded.last_seen_at",
            params![
                record.device_id.trim(),
                record.user_id.trim(),
                record.client.trim(),
                record.name.trim(),
                record.os,
                record.arch,
                record.app_version,
                record.last_seen_at,
                record.created_at,
                record.revoked as i64,
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
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT device_id, user_id, client, name, os, arch, app_version, last_seen_at, created_at, revoked \
                 FROM cloud_devices WHERE device_id = ?",
                params![cleaned],
                |row| Self::read_cloud_device(row),
            )
            .optional()?;
        Ok(row)
    }

    fn touch_cloud_device_impl(&self, device_id: &str, last_seen_at: f64) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "UPDATE cloud_devices SET last_seen_at = ? WHERE device_id = ?",
            params![last_seen_at, cleaned],
        )?;
        Ok(())
    }

    fn set_cloud_device_revoked_impl(&self, device_id: &str, revoked: bool) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = device_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        self.open()?.execute(
            "UPDATE cloud_devices SET revoked = ? WHERE device_id = ?",
            params![revoked as i64, cleaned],
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
            &format!("SELECT COUNT(*) FROM cloud_devices{where_clause}"),
            params_from_iter(params_list.iter()),
            |row| row.get(0),
        )?;
        let mut rows = Vec::new();
        if limit > 0 {
            let mut sql = format!(
                "SELECT device_id, user_id, client, name, os, arch, app_version, last_seen_at, created_at, revoked \
                 FROM cloud_devices{where_clause} ORDER BY last_seen_at DESC"
            );
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
            let mut stmt = conn.prepare(&sql)?;
            rows = stmt
                .query_map(params_from_iter(params_list.iter()), |row| {
                    Self::read_cloud_device(row)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        Ok((rows, total))
    }

    fn insert_cloud_call_record_impl(&self, record: &CloudCallRecord) -> Result<()> {
        self.ensure_initialized()?;
        self.open()?.execute(
            "INSERT INTO cloud_call_records(call_id, device_id, user_id, client, local_session_id, model, provider, status, quota_consumed, queue_waited_ms, prompt_tokens, completion_tokens, started_at, finished_at, duration_ms, error_summary) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                record.call_id.trim(),
                record.device_id.trim(),
                record.user_id.trim(),
                record.client.trim(),
                record.local_session_id,
                record.model.trim(),
                record.provider,
                record.status.trim(),
                record.quota_consumed as i64,
                record.queue_waited_ms,
                record.prompt_tokens,
                record.completion_tokens,
                record.started_at,
                record.finished_at,
                record.duration_ms,
                record.error_summary,
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
        self.open()?.execute(
            "UPDATE cloud_call_records \
             SET status = ?, prompt_tokens = ?, completion_tokens = ?, finished_at = ?, \
                 duration_ms = CAST(MAX((? - started_at) * 1000.0, 0) AS INTEGER), error_summary = ? \
             WHERE call_id = ?",
            params![
                status.trim(),
                prompt_tokens,
                completion_tokens,
                finished_at,
                finished_at,
                error_summary,
                cleaned,
            ],
        )?;
        Ok(())
    }

    fn list_cloud_call_records_impl(
        &self,
        query: ListCloudRecordsQuery<'_>,
    ) -> Result<(Vec<CloudCallRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        for (column, value) in [
            ("user_id", query.user_id),
            ("device_id", query.device_id),
            ("model", query.model),
            ("status", query.status),
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
            &format!("SELECT COUNT(*) FROM cloud_call_records{where_clause}"),
            params_from_iter(params_list.iter()),
            |row| row.get(0),
        )?;
        let mut rows = Vec::new();
        if query.limit > 0 {
            let mut sql = format!(
                "SELECT call_id, device_id, user_id, client, local_session_id, model, provider, status, quota_consumed, queue_waited_ms, prompt_tokens, completion_tokens, started_at, finished_at, duration_ms, error_summary \
                 FROM cloud_call_records{where_clause} ORDER BY started_at DESC"
            );
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(query.limit));
            params_list.push(SqlValue::from(query.offset.max(0)));
            let mut stmt = conn.prepare(&sql)?;
            rows = stmt
                .query_map(params_from_iter(params_list.iter()), |row| {
                    Ok(CloudCallRecord {
                        call_id: row.get(0)?,
                        device_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        user_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        client: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        local_session_id: row.get(4)?,
                        model: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        provider: row.get(6)?,
                        status: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                        quota_consumed: row.get::<_, i64>(8)? != 0,
                        queue_waited_ms: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
                        prompt_tokens: row.get(10)?,
                        completion_tokens: row.get(11)?,
                        started_at: row.get::<_, Option<f64>>(12)?.unwrap_or(0.0),
                        finished_at: row.get(13)?,
                        duration_ms: row.get(14)?,
                        error_summary: row.get(15)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        Ok((rows, total))
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
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut accepted = 0_i64;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO cloud_device_logs(device_id, seq, user_id, client, level, category, event, message, local_session_id, created_at) \
                 VALUES(?,?,?,?,?,?,?,?,?,?)",
            )?;
            for record in records {
                let inserted = stmt.execute(params![
                    record.device_id.trim(),
                    record.seq,
                    record.user_id.trim(),
                    record.client.trim(),
                    record.level.trim(),
                    record.category.trim(),
                    record.event.trim(),
                    record.message,
                    record.local_session_id,
                    record.created_at,
                ])?;
                accepted += inserted as i64;
            }
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
        let conn = self.open()?;
        let latest: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM cloud_device_logs WHERE device_id = ?",
            params![cleaned],
            |row| row.get(0),
        )?;
        Ok(latest)
    }

    fn list_cloud_device_logs_impl(
        &self,
        query: ListCloudDeviceLogsQuery<'_>,
    ) -> Result<(Vec<CloudDeviceLogRecord>, i64)> {
        self.ensure_initialized()?;
        let mut conditions = Vec::new();
        let mut params_list: Vec<SqlValue> = Vec::new();
        for (column, value) in [
            ("user_id", query.user_id),
            ("device_id", query.device_id),
            ("level", query.level),
            ("category", query.category),
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
            &format!("SELECT COUNT(*) FROM cloud_device_logs{where_clause}"),
            params_from_iter(params_list.iter()),
            |row| row.get(0),
        )?;
        let mut rows = Vec::new();
        if query.limit > 0 {
            // The (device_id, seq) primary key serves this ordering directly.
            let mut sql = format!(
                "SELECT device_id, seq, user_id, client, level, category, event, message, local_session_id, created_at \
                 FROM cloud_device_logs{where_clause} ORDER BY device_id DESC, seq DESC"
            );
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(query.limit));
            params_list.push(SqlValue::from(query.offset.max(0)));
            let mut stmt = conn.prepare(&sql)?;
            rows = stmt
                .query_map(params_from_iter(params_list.iter()), |row| {
                    Self::read_cloud_device_log(row)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
        }
        Ok((rows, total))
    }

    fn cleanup_cloud_device_logs_impl(&self, retention_days: u32) -> Result<u64> {
        self.ensure_initialized()?;
        // Zero retention means "never expire": log cleanup is disabled.
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Self::now_ts() - (retention_days as f64) * 86_400.0;
        let deleted = self.open()?.execute(
            "DELETE FROM cloud_device_logs WHERE created_at < ?",
            params![cutoff],
        )?;
        Ok(deleted as u64)
    }
}

impl SqliteStorage {
    fn read_cloud_device(row: &rusqlite::Row<'_>) -> rusqlite::Result<CloudDeviceRecord> {
        Ok(CloudDeviceRecord {
            device_id: row.get(0)?,
            user_id: row.get(1)?,
            client: row.get(2)?,
            name: row.get(3)?,
            os: row.get(4)?,
            arch: row.get(5)?,
            app_version: row.get(6)?,
            last_seen_at: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
            created_at: row.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
            revoked: row.get::<_, i64>(9)? != 0,
            interlink: None,
        })
    }

    fn read_cloud_device_log(row: &rusqlite::Row<'_>) -> rusqlite::Result<CloudDeviceLogRecord> {
        Ok(CloudDeviceLogRecord {
            device_id: row.get(0)?,
            seq: row.get(1)?,
            user_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            client: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            level: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            category: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            event: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            message: row.get(7)?,
            local_session_id: row.get(8)?,
            created_at: row.get::<_, Option<f64>>(9)?.unwrap_or(0.0),
        })
    }
}
