use super::PostgresStorage;
use crate::storage::{
    ChannelMessageRecord, ChannelMessageStats, ChannelOutboxRecord, ChannelOutboxStats,
    ChannelSessionRecord, StorageLifecycle, UpdateChannelOutboxStatusParams,
};
use anyhow::Result;
use serde_json::Value;
use tokio_postgres::types::ToSql;

pub(super) trait PostgresChannelRuntimeStorage {
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

impl PostgresChannelRuntimeStorage for PostgresStorage {
    fn upsert_channel_session_impl(&self, record: &ChannelSessionRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let thread_id = Self::normalize_channel_thread_id(record.thread_id.as_deref());
        let metadata = record.metadata.as_ref().map(Self::json_to_string);
        let tts_enabled = record.tts_enabled.map(|value| if value { 1 } else { 0 });
        conn.execute(
            "INSERT INTO channel_sessions (channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(channel, account_id, peer_kind, peer_id, thread_id) DO UPDATE SET session_id = EXCLUDED.session_id, agent_id = EXCLUDED.agent_id, user_id = EXCLUDED.user_id, \
             tts_enabled = EXCLUDED.tts_enabled, tts_voice = EXCLUDED.tts_voice, metadata = EXCLUDED.metadata, last_message_at = EXCLUDED.last_message_at, updated_at = EXCLUDED.updated_at",
            &[
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &thread_id,
                &record.session_id,
                &record.agent_id,
                &record.user_id,
                &tts_enabled,
                &record.tts_voice,
                &metadata,
                &record.last_message_at,
                &record.created_at,
                &record.updated_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at \
             FROM channel_sessions WHERE channel = $1 AND account_id = $2 AND peer_kind = $3 AND peer_id = $4 AND thread_id IS NOT DISTINCT FROM $5",
            &[
                &cleaned_channel,
                &cleaned_account,
                &cleaned_peer_kind,
                &cleaned_peer_id,
                &thread_id,
            ],
        )?;
        Ok(row.map(|row| ChannelSessionRecord {
            channel: row.get(0),
            account_id: row.get(1),
            peer_kind: row.get(2),
            peer_id: row.get(3),
            thread_id: Self::normalize_channel_thread_value(row.get(4)),
            session_id: row.get(5),
            agent_id: row.get(6),
            user_id: row.get(7),
            tts_enabled: row.get::<_, Option<i32>>(8).map(|value| value != 0),
            tts_voice: row.get(9),
            metadata: row
                .get::<_, Option<String>>(10)
                .and_then(|value| Self::json_from_str(&value)),
            last_message_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
            created_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
            updated_at: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
        }))
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
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            filters.push(format!("channel = ${}", params.len()));
        }
        if let Some(account) = account_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(account.to_string()));
            filters.push(format!("account_id = ${}", params.len()));
        }
        if let Some(peer_id) = peer_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(peer_id.to_string()));
            filters.push(format!("peer_id = ${}", params.len()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(session_id.to_string()));
            filters.push(format!("session_id = ${}", params.len()));
        }
        let mut query =
            "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, agent_id, user_id, tts_enabled, tts_voice, metadata, last_message_at, created_at, updated_at FROM channel_sessions"
                .to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY updated_at DESC");
        let offset_value = offset.max(0);
        let limit_value = if limit <= 0 { 100 } else { limit.min(500) };
        params.push(Box::new(offset_value));
        params.push(Box::new(limit_value));
        query.push_str(&format!(
            " OFFSET ${} LIMIT ${}",
            params.len() - 1,
            params.len()
        ));
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelSessionRecord {
                channel: row.get(0),
                account_id: row.get(1),
                peer_kind: row.get(2),
                peer_id: row.get(3),
                thread_id: Self::normalize_channel_thread_value(row.get(4)),
                session_id: row.get(5),
                agent_id: row.get(6),
                user_id: row.get(7),
                tts_enabled: row.get::<_, Option<i32>>(8).map(|value| value != 0),
                tts_voice: row.get(9),
                metadata: row
                    .get::<_, Option<String>>(10)
                    .and_then(|value| Self::json_from_str(&value)),
                last_message_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                created_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(13).unwrap_or(0.0),
            });
        }
        let mut count_query = "SELECT COUNT(*) FROM channel_sessions".to_string();
        if !filters.is_empty() {
            count_query.push_str(" WHERE ");
            count_query.push_str(&filters.join(" AND "));
        }
        let count_params: Vec<&(dyn ToSql + Sync)> = params_refs[..params_refs.len() - 2].to_vec();
        let total_row = conn.query_one(&count_query, &count_params)?;
        let total: i64 = total_row.get(0);
        Ok((output, total))
    }

    fn insert_channel_message_impl(&self, record: &ChannelMessageRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let payload = Self::json_to_string(&record.payload);
        let raw_payload = record.raw_payload.as_ref().map(Self::json_to_string);
        conn.execute(
            "INSERT INTO channel_messages (channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
            &[
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.thread_id,
                &record.session_id,
                &record.message_id,
                &record.sender_id,
                &record.message_type,
                &payload,
                &raw_payload,
                &record.created_at,
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
        let mut conn = self.conn()?;
        let mut filters = Vec::new();
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
        if let Some(channel) = channel
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(channel.to_string()));
            filters.push(format!("channel = ${}", params.len()));
        }
        if let Some(session_id) = session_id
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            params.push(Box::new(session_id.to_string()));
            filters.push(format!("session_id = ${}", params.len()));
        }
        let mut query = "SELECT channel, account_id, peer_kind, peer_id, thread_id, session_id, message_id, sender_id, message_type, payload, raw_payload, created_at \
             FROM channel_messages"
            .to_string();
        if !filters.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&filters.join(" AND "));
        }
        query.push_str(" ORDER BY id DESC");
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        params.push(Box::new(limit_value));
        query.push_str(&format!(" LIMIT ${}", params.len()));
        let params_refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|p| p.as_ref()).collect();
        let rows = conn.query(&query, &params_refs)?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelMessageRecord {
                channel: row.get(0),
                account_id: row.get(1),
                peer_kind: row.get(2),
                peer_id: row.get(3),
                thread_id: row.get(4),
                session_id: row.get(5),
                message_id: row.get(6),
                sender_id: row.get(7),
                message_type: row.get(8),
                payload: Self::json_from_str(row.get::<_, String>(9).as_str())
                    .unwrap_or(Value::Null),
                raw_payload: row
                    .get::<_, Option<String>>(10)
                    .and_then(|value| Self::json_from_str(&value)),
                created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
            });
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
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT COUNT(*)::BIGINT, MAX(created_at) FROM channel_messages WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(ChannelMessageStats {
            total: row.get::<_, i64>(0),
            last_message_at: row.get::<_, Option<f64>>(1),
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
        let mut conn = self.conn()?;
        let row = conn.query_one(
            "SELECT \
                COUNT(*)::BIGINT, \
                SUM(CASE WHEN status = 'sent' THEN 1 ELSE 0 END)::BIGINT, \
                SUM(CASE WHEN status = 'retry' THEN 1 ELSE 0 END)::BIGINT, \
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END)::BIGINT, \
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END)::BIGINT, \
                SUM(COALESCE(retry_count, 0))::BIGINT, \
                MAX(CASE WHEN status = 'sent' THEN COALESCE(delivered_at, updated_at, created_at) END), \
                MAX(CASE WHEN status = 'failed' THEN COALESCE(updated_at, created_at) END) \
             FROM channel_outbox WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(ChannelOutboxStats {
            total: row.get::<_, i64>(0),
            sent: row.get::<_, Option<i64>>(1).unwrap_or(0),
            retry: row.get::<_, Option<i64>>(2).unwrap_or(0),
            pending: row.get::<_, Option<i64>>(3).unwrap_or(0),
            failed: row.get::<_, Option<i64>>(4).unwrap_or(0),
            retry_attempts: row.get::<_, Option<i64>>(5).unwrap_or(0),
            last_sent_at: row.get::<_, Option<f64>>(6),
            last_failed_at: row.get::<_, Option<f64>>(7),
        })
    }

    fn delete_channel_sessions_impl(&self, channel: &str, account_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_channel = channel.trim();
        let cleaned_account = account_id.trim();
        if cleaned_channel.is_empty() || cleaned_account.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_sessions WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
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
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_messages WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
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
        let mut conn = self.conn()?;
        let affected = conn.execute(
            "DELETE FROM channel_outbox WHERE channel = $1 AND account_id = $2",
            &[&cleaned_channel, &cleaned_account],
        )?;
        Ok(affected as i64)
    }

    fn enqueue_channel_outbox_impl(&self, record: &ChannelOutboxRecord) -> Result<()> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let payload = Self::json_to_string(&record.payload);
        conn.execute(
            "INSERT INTO channel_outbox (outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT(outbox_id) DO UPDATE SET payload = EXCLUDED.payload, status = EXCLUDED.status, retry_count = EXCLUDED.retry_count, retry_at = EXCLUDED.retry_at, \
             last_error = EXCLUDED.last_error, updated_at = EXCLUDED.updated_at, delivered_at = EXCLUDED.delivered_at",
            &[
                &record.outbox_id,
                &record.channel,
                &record.account_id,
                &record.peer_kind,
                &record.peer_id,
                &record.thread_id,
                &payload,
                &record.status,
                &record.retry_count,
                &record.retry_at,
                &record.last_error,
                &record.created_at,
                &record.updated_at,
                &record.delivered_at,
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
             FROM channel_outbox WHERE outbox_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| ChannelOutboxRecord {
            outbox_id: row.get(0),
            channel: row.get(1),
            account_id: row.get(2),
            peer_kind: row.get(3),
            peer_id: row.get(4),
            thread_id: row.get(5),
            payload: Self::json_from_str(row.get::<_, String>(6).as_str()).unwrap_or(Value::Null),
            status: row.get(7),
            retry_count: row.get(8),
            retry_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
            last_error: row.get(10),
            created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
            updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
            delivered_at: row.get(13),
        }))
    }

    fn list_pending_channel_outbox_impl(&self, limit: i64) -> Result<Vec<ChannelOutboxRecord>> {
        self.ensure_initialized()?;
        let mut conn = self.conn()?;
        let now = Self::now_ts();
        let limit_value = if limit <= 0 { 50 } else { limit.min(200) };
        let rows = conn.query(
            "SELECT outbox_id, channel, account_id, peer_kind, peer_id, thread_id, payload, status, retry_count, retry_at, last_error, created_at, updated_at, delivered_at \
             FROM channel_outbox WHERE (status = 'pending' OR status = 'retry') AND retry_at <= $1 \
             ORDER BY retry_at ASC LIMIT $2",
            &[&now, &limit_value],
        )?;
        let mut output = Vec::new();
        for row in rows {
            output.push(ChannelOutboxRecord {
                outbox_id: row.get(0),
                channel: row.get(1),
                account_id: row.get(2),
                peer_kind: row.get(3),
                peer_id: row.get(4),
                thread_id: row.get(5),
                payload: Self::json_from_str(row.get::<_, String>(6).as_str())
                    .unwrap_or(Value::Null),
                status: row.get(7),
                retry_count: row.get(8),
                retry_at: row.get::<_, Option<f64>>(9).unwrap_or(0.0),
                last_error: row.get(10),
                created_at: row.get::<_, Option<f64>>(11).unwrap_or(0.0),
                updated_at: row.get::<_, Option<f64>>(12).unwrap_or(0.0),
                delivered_at: row.get(13),
            });
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
        let mut conn = self.conn()?;
        conn.execute(
            "UPDATE channel_outbox SET status = $1, retry_count = $2, retry_at = $3, last_error = $4, updated_at = $5, delivered_at = $6 WHERE outbox_id = $7",
            &[
                &params.status,
                &params.retry_count,
                &params.retry_at,
                &params.last_error,
                &params.updated_at,
                &params.delivered_at,
                &cleaned,
            ],
        )?;
        Ok(())
    }
}
