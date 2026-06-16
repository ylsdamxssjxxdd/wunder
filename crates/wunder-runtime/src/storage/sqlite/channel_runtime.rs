use super::SqliteStorage;
use crate::storage::{
    ChannelMessageRecord, ChannelMessageStats, ChannelOutboxRecord, ChannelOutboxStats,
    ChannelSessionRecord, StorageLifecycle, UpdateChannelOutboxStatusParams,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension};
use serde_json::Value;

pub(super) trait SqliteChannelRuntimeStorage {
    fn upsert_channel_session_impl(&self, record: &ChannelSessionRecord) -> Result<()>;
    fn get_channel_session_impl(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>>;
    fn list_channel_sessions_impl(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        peer_id: Option<&str>,
        session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChannelSessionRecord>, i64)>;
    fn insert_channel_message_impl(&self, record: &ChannelMessageRecord) -> Result<()>;
    fn list_channel_messages_impl(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>>;
    fn get_channel_message_stats_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelMessageStats>;
    fn get_channel_outbox_stats_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelOutboxStats>;
    fn delete_channel_sessions_impl(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn delete_channel_messages_impl(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn delete_channel_outbox_impl(&self, channel: &str, account_id: &str) -> Result<i64>;
    fn enqueue_channel_outbox_impl(&self, record: &ChannelOutboxRecord) -> Result<()>;
    fn get_channel_outbox_impl(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>>;
    fn list_pending_channel_outbox_impl(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>>;
    fn update_channel_outbox_status_impl(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()>;
}

impl SqliteChannelRuntimeStorage for SqliteStorage {
    fn upsert_channel_session_impl(&self, record: &ChannelSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let thread_id = Self::normalize_channel_thread_id(record.thread_id.as_deref());
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        let tts_enabled = record.tts_enabled.map(|value| if value { 1 } else { 0 });
        conn.execute(
            "INSERT INTO channel_sessions (channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id, thread_id) DO UPDATE SET session_id = excluded.session_id, agent_id = excluded.agent_id, user_id = excluded.user_id, \
             tts_enabled = excluded.tts_enabled, tts_voice = excluded.tts_voice, metadata = excluded.metadata, last_message_at = excluded.last_message_at, updated_at = excluded.updated_at",
            params![
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                thread_id,
                record.session_id,
                record.agent_id,
                record.user_id,
                tts_enabled,
                record.tts_voice,
                metadata,
                record.last_message_at,
                record.created_at,
                record.updated_at
            ],
        )?;
        Ok(())
    }

    fn get_channel_session_impl(
        &self,
        channel: &str,
        account_id: &str,
        peer_kind: &str,
        peer_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Option<ChannelSessionRecord>> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        let cleaned_peer_kind = peer_kind.trim();
        let cleaned_peer_id = peer_id.trim();
        if cleaned_channel.is_empty()
            || cleaned_account.is_empty()
            || cleaned_peer_kind.is_empty()
            || cleaned_peer_id.is_empty()
        {
            return Ok(None);
        }
        let thread_id = Self::normalize_channel_thread_id(thread_id);
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at \
                 FROM channel_sessions WHERE channel = ? AND account_id = ? AND peer_kind = ? AND peer_id = ? AND (thread_id IS ? OR thread_id = ?)",
                params![
                    cleaned_channel,
                    cleaned_account,
                    cleaned_peer_kind,
                    cleaned_peer_id,
                    thread_id,
                    thread_id
                ],
                |row| {
                    let metadata_text: Option<String> = row.get(10)?;
                    Ok(ChannelSessionRecord {
                        channel: row.get(0)?,
                        account_id: row.get(1)?,
                        peer_kind: row.get(2)?,
                        peer_id: row.get(3)?,
                        thread_id: Self::normalize_channel_thread_value(row.get(4)?),
                        session_id: row.get(5)?,
                        agent_id: row.get(6)?,
                        user_id: row.get(7)?,
                        tts_enabled: row.get::<_, Option<i64>>(8)?.map(|value| value != 0),
                        tts_voice: row.get(9)?,
                        metadata: metadata_text.and_then(|value| Self::json_from_str(&value)),
                        last_message_at: row.get(11)?,
                        created_at: row.get(12)?,
                        updated_at: row.get(13)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_channel_sessions_impl(
        &self,
        channel: Option<&str>,
        account_id: Option<&str>,
        peer_id: Option<&str>,
        session_id: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ChannelSessionRecord>, i64)> {
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
        if let Some(account) = account_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("account_id = ?".to_string());
            params_list.push(SqlValue::from(account.to_string()));
        }
        if let Some(peer_id) = peer_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("peer_id = ?".to_string());
            params_list.push(SqlValue::from(peer_id.to_string()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("session_id = ?".to_string());
            params_list.push(SqlValue::from(session_id.to_string()));
        }
        let mut query = "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at FROM channel_sessions".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let offset_value = offset.max(0);
        let limit_value = if limit <= 0 { 100 } else { limit.min(500) };
        params_list.push(SqlValue::from(limit_value));
        params_list.push(SqlValue::from(offset_value));
        query.push_str(" LIMIT ? OFFSET ?");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let metadata_text: Option<String> = row.get(10)?;
            Ok(ChannelSessionRecord {
                channel: row.get(0)?,
                account_id: row.get(1)?,
                peer_kind: row.get(2)?,
                peer_id: row.get(3)?,
                thread_id: Self::normalize_channel_thread_value(row.get(4)?),
                session_id: row.get(5)?,
                agent_id: row.get(6)?,
                user_id: row.get(7)?,
                tts_enabled: row.get::<_, Option<i64>>(8)?.map(|value| value != 0),
                tts_voice: row.get(9)?,
                metadata: metadata_text.and_then(|value| Self::json_from_str(&value)),
                last_message_at: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }

        let mut count_query = "SELECT COUNT(*) FROM channel_sessions".to_string();
        if !filters.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&filters.join(" AND "));
        }
        let count_params = params_from_iter(params_list.iter().take(params_list.len() - 2));
        let total: i64 = conn.query_row(&count_query, count_params, |row| row.get(0))?;
        Ok((output, total))
    }

    fn insert_channel_message_impl(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let payload = Self::json_to_string(&record.payload);
        let raw_payload = record.raw_payload.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO channel_messages (channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.thread_id,
                record.session_id,
                record.message_id,
                record.sender_id,
                record.message_type,
                payload,
                raw_payload,
                record.created_at
            ],
        )?;
        Ok(())
    }

    fn list_channel_messages_impl(
        &self,
        channel: Option<&str>,
        session_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ChannelMessageRecord>> {
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
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            filters.push("session_id = ?".to_string());
            params_list.push(SqlValue::from(session_id.to_string()));
        }
        let mut query = "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at FROM channel_messages".to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY id DESC");
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        params_list.push(SqlValue::from(limit_value));
        query.push_str(" LIMIT ?");
        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map(params_from_iter(params_list.iter()), |row| {
            let payload_text: String = row.get(9)?;
            let raw_text: Option<String> = row.get(10)?;
            Ok(ChannelMessageRecord {
                channel: row.get(0)?,
                account_id: row.get(1)?,
                peer_kind: row.get(2)?,
                peer_id: row.get(3)?,
                thread_id: row.get(4)?,
                session_id: row.get(5)?,
                message_id: row.get(6)?,
                sender_id: row.get(7)?,
                message_type: row.get(8)?,
                payload: Self::json_from_str(&payload_text).unwrap_or(Value::Null),
                raw_payload: raw_text.and_then(|value| Self::json_from_str(&value)),
                created_at: row.get(11)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn get_channel_message_stats_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelMessageStats> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(ChannelMessageStats::default());
        }
        let conn = self.open()?;
        let (total, last_message_at): (i64, Option<f64>) = conn.query_row(
            "SELECT COUNT(*), MAX(created_at) FROM channel_messages WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(ChannelMessageStats {
            total,
            last_message_at,
        })
    }

    fn get_channel_outbox_stats_impl(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelOutboxStats> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(ChannelOutboxStats::default());
        }
        let conn = self.open()?;
        let (
            total,
            sent,
            retry,
            pending,
            failed,
            retry_attempts,
            last_sent_at,
            last_failed_at,
        ): (
            i64,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<f64>,
            Option<f64>,
        ) = conn.query_row(
            "SELECT \
                COUNT(*), \
                SUM(CASE WHEN status = 'sent' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'retry' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), \
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), \
                SUM(COALESCE(retry_count, 0)), \
                MAX(CASE WHEN status = 'sent' THEN COALESCE(delivered_at, updated_at, created_at) END), \
                MAX(CASE WHEN status = 'failed' THEN COALESCE(updated_at, created_at) END) \
             FROM channel_outbox WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )?;
        Ok(ChannelOutboxStats {
            total,
            sent: sent.unwrap_or(0),
            retry: retry.unwrap_or(0),
            pending: pending.unwrap_or(0),
            failed: failed.unwrap_or(0),
            retry_attempts: retry_attempts.unwrap_or(0),
            last_sent_at,
            last_failed_at,
        })
    }

    fn delete_channel_sessions_impl(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_sessions WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn delete_channel_messages_impl(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_messages WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn delete_channel_outbox_impl(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        let affected = conn.execute(
            "DELETE FROM channel_outbox WHERE channel = ? AND account_id = ?",
            params![cleaned_channel, cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn enqueue_channel_outbox_impl(&self, record: &ChannelOutboxRecord) -> Result<()> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let payload = Self::json_to_string(&record.payload);
        conn.execute(
            "INSERT INTO channel_outbox (outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(outbox_id) DO UPDATE SET payload = excluded.payload, status = excluded.status, retry_count = excluded.retry_count, retry_at = excluded.retry_at, \
             last_error = excluded.last_error, updated_at = excluded.updated_at, delivered_at = excluded.delivered_at",
            params![
                record.outbox_id,
                record.channel,
                record.account_id,
                record.peer_kind,
                record.peer_id,
                record.thread_id,
                payload,
                record.status,
                record.retry_count,
                record.retry_at,
                record.last_error,
                record.created_at,
                record.updated_at,
                record.delivered_at
            ],
        )?;
        Ok(())
    }

    fn get_channel_outbox_impl(&self, outbox_id: &str) -> Result<Option<ChannelOutboxRecord>> {
        self.ensure_initialized()?;
        let cleaned = outbox_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
                 FROM channel_outbox WHERE outbox_id = ?",
                params![cleaned],
                |row| {
                    let payload_text: String = row.get(6)?;
                    Ok(ChannelOutboxRecord {
                        outbox_id: row.get(0)?,
                        channel: row.get(1)?,
                        account_id: row.get(2)?,
                        peer_kind: row.get(3)?,
                        peer_id: row.get(4)?,
                        thread_id: row.get(5)?,
                        payload: Self::json_from_str(&payload_text).unwrap_or(Value::Null),
                        status: row.get(7)?,
                        retry_count: row.get(8)?,
                        retry_at: row.get(9)?,
                        last_error: row.get(10)?,
                        created_at: row.get(11)?,
                        updated_at: row.get(12)?,
                        delivered_at: row.get(13)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    fn list_pending_channel_outbox_impl(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        self.ensure_initialized()?;
        let conn = self.open()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let mut stmt = conn.prepare(
            "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
             FROM channel_outbox WHERE (status = 'pending' OR status = 'retry') AND retry_at <= ? ORDER BY retry_at ASC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![now, limit_value], |row| {
            let payload_text: String = row.get(6)?;
            Ok(ChannelOutboxRecord {
                outbox_id: row.get(0)?,
                channel: row.get(1)?,
                account_id: row.get(2)?,
                peer_kind: row.get(3)?,
                peer_id: row.get(4)?,
                thread_id: row.get(5)?,
                payload: Self::json_from_str(&payload_text).unwrap_or(Value::Null),
                status: row.get(7)?,
                retry_count: row.get(8)?,
                retry_at: row.get(9)?,
                last_error: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                delivered_at: row.get(13)?,
            })
        })?;
        let mut output = Vec::new();
        for record in rows.flatten() {
            output.push(record);
        }
        Ok(output)
    }

    fn update_channel_outbox_status_impl(
        &self,
        params: UpdateChannelOutboxStatusParams<'_>,
    ) -> Result<()> {
        self.ensure_initialized()?;
        let cleaned = params.outbox_id.trim();
        if cleaned.is_empty() {
            return Ok(());
        }
        let conn = self.open()?;
        conn.execute(
            "UPDATE channel_outbox SET status = ?, retry_count = ?, retry_at = ?, last_error = ?, updated_at = ?, delivered_at = ? WHERE outbox_id = ?",
            params![
                params.status,
                params.retry_count,
                params.retry_at,
                params.last_error,
                params.updated_at,
                params.delivered_at,
                cleaned
            ],
        )?;
        Ok(())
    }
}
