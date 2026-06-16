use super::SqliteStorage;
use crate::storage::{
    BeeroomChatMessageRecord, StorageLifecycle, UserWorldConversationRecord,
    UserWorldConversationSummaryRecord, UserWorldEventRecord, UserWorldGroupRecord,
    UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult, UserWorldSendMessageResult,
};
use anyhow::Result;
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, OptionalExtension, Row, TransactionBehavior};
use serde_json::{json, Value};
use std::collections::HashSet;

pub(super) trait SqliteUserWorldStorage {
    fn resolve_or_create_user_world_direct_conversation_impl(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord>;
    fn create_user_world_group_impl(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationRecord>;
    fn get_user_world_conversation_impl(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>>;
    fn get_user_world_member_impl(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>>;
    fn list_user_world_conversations_impl(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)>;
    fn list_user_world_messages_impl(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>>;
    fn send_user_world_message_impl(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendMessageResult>;
    fn mark_user_world_read_impl(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>>;
    fn list_user_world_events_impl(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>>;
    fn list_user_world_groups_impl(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupRecord>, i64)>;
    fn get_user_world_group_by_id_impl(
        &self,
        group_id: &str,
    ) -> Result<Option<UserWorldGroupRecord>>;
    fn update_user_world_group_announcement_impl(
        &self,
        group_id: &str,
        announcement: Option<&str>,
        announcement_updated_at: Option<f64>,
        updated_at: f64,
    ) -> Result<Option<UserWorldGroupRecord>>;
    fn list_user_world_member_user_ids_impl(&self, conversation_id: &str) -> Result<Vec<String>>;
    fn list_beeroom_chat_messages_impl(
        &self,
        user_id: &str,
        group_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<BeeroomChatMessageRecord>>;
    fn append_beeroom_chat_message_impl(
        &self,
        user_id: &str,
        group_id: &str,
        sender_kind: &str,
        sender_name: &str,
        sender_agent_id: Option<&str>,
        mention_name: Option<&str>,
        mention_agent_id: Option<&str>,
        body: &str,
        meta: Option<&str>,
        tone: &str,
        client_msg_id: Option<&str>,
        created_at: f64,
    ) -> Result<BeeroomChatMessageRecord>;
    fn delete_beeroom_chat_messages_impl(&self, user_id: &str, group_id: &str) -> Result<i64>;
}

impl SqliteUserWorldStorage for SqliteStorage {
    fn resolve_or_create_user_world_direct_conversation_impl(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.ensure_initialized()?;
        let (participant_a, participant_b) = normalize_user_world_pair(user_a, user_b)
            .ok_or_else(|| anyhow::anyhow!("invalid user pair"))?;
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing = tx
            .query_row(
                "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
                 NULL AS group_id, NULL AS group_name, 2 AS member_count, \
                 c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_conversations c \
                 WHERE c.conversation_type = 'direct' AND c.participant_a = ? AND c.participant_b = ?",
                params![participant_a, participant_b],
                map_user_world_conversation_row,
            )
            .optional()?;
        if let Some(record) = existing {
            tx.commit()?;
            return Ok(record);
        }

        let conversation_id = format!("uwc_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO user_world_conversations (conversation_id, conversation_type, participant_a, participant_b, \
             created_at, updated_at, last_message_at, last_message_id, last_message_preview) \
             VALUES (?, 'direct', ?, ?, ?, ?, ?, NULL, NULL)",
            params![
                conversation_id,
                participant_a,
                participant_b,
                now,
                now,
                now
            ],
        )?;
        tx.execute(
            "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
             pinned, muted, updated_at) VALUES (?, ?, ?, NULL, 0, 0, 0, ?)",
            params![conversation_id, participant_a, participant_b, now],
        )?;
        tx.execute(
            "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
             pinned, muted, updated_at) VALUES (?, ?, ?, NULL, 0, 0, 0, ?)",
            params![conversation_id, participant_b, participant_a, now],
        )?;
        let record = tx
            .query_row(
                "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
                 NULL AS group_id, NULL AS group_name, 2 AS member_count, \
                 c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_conversations c WHERE c.conversation_id = ?",
                params![conversation_id],
                map_user_world_conversation_row,
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("user world conversation missing after insert"))?;
        tx.commit()?;
        Ok(record)
    }

    fn create_user_world_group_impl(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.ensure_initialized()?;
        let owner = owner_user_id.trim();
        let name = group_name.trim();
        if owner.is_empty() || name.is_empty() {
            return Err(anyhow::anyhow!("owner_user_id/group_name is required"));
        }
        let members = normalize_user_world_members(owner, member_user_ids);
        if members.len() < 2 {
            return Err(anyhow::anyhow!("group requires at least 2 users"));
        }
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let conversation_id = format!("uwc_{}", uuid::Uuid::new_v4().simple());
        let group_id = format!("uwg_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO user_world_conversations (conversation_id, conversation_type, participant_a, participant_b, \
             created_at, updated_at, last_message_at, last_message_id, last_message_preview) \
             VALUES (?, 'group', ?, ?, ?, ?, ?, NULL, NULL)",
            params![conversation_id, owner, group_id, now, now, now],
        )?;
        tx.execute(
            "INSERT INTO user_world_groups (group_id, conversation_id, group_name, owner_user_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params![group_id, conversation_id, name, owner, now, now],
        )?;
        for member_user_id in &members {
            tx.execute(
                "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
                 pinned, muted, updated_at) VALUES (?, ?, '', NULL, 0, 0, 0, ?)",
                params![conversation_id, member_user_id, now],
            )?;
        }
        let member_count = members.len() as i64;
        let record = tx
            .query_row(
                "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
                 g.group_id, g.group_name, ? AS member_count, \
                 c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_conversations c \
                 JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
                 WHERE c.conversation_id = ?",
                params![member_count, conversation_id],
                map_user_world_conversation_row,
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("user world group missing after insert"))?;
        tx.commit()?;
        Ok(record)
    }

    fn get_user_world_conversation_impl(
        &self,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationRecord>> {
        self.ensure_initialized()?;
        let cleaned = conversation_id.trim();
        if cleaned.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
                 g.group_id, g.group_name, \
                 CASE WHEN c.conversation_type = 'group' THEN \
                    (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = c.conversation_id) \
                 ELSE NULL END AS member_count, \
                 c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_conversations c \
                 LEFT JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
                 WHERE c.conversation_id = ?",
                params![cleaned],
                map_user_world_conversation_row,
            )
            .optional()?;
        Ok(row)
    }

    fn get_user_world_member_impl(
        &self,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<Option<UserWorldMemberRecord>> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        let cleaned_user = user_id.trim();
        if cleaned_conversation.is_empty() || cleaned_user.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        let row = conn
            .query_row(
                "SELECT conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, pinned, muted, updated_at \
                 FROM user_world_members WHERE conversation_id = ? AND user_id = ?",
                params![cleaned_conversation, cleaned_user],
                map_user_world_member_row,
            )
            .optional()?;
        Ok(row)
    }

    fn list_user_world_conversations_impl(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationSummaryRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM user_world_members WHERE user_id = ?",
            params![cleaned_user],
            |row| row.get(0),
        )?;

        let mut sql = "SELECT c.conversation_id, c.conversation_type, m.peer_user_id, \
                       g.group_id, g.group_name, \
                       CASE WHEN c.conversation_type = 'group' THEN \
                         (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = c.conversation_id) \
                       ELSE NULL END AS member_count, \
                       m.last_read_message_id, m.unread_count_cache, m.pinned, m.muted, m.updated_at, \
                       c.last_message_at, c.last_message_id, c.last_message_preview \
                       FROM user_world_members m \
                       JOIN user_world_conversations c ON c.conversation_id = m.conversation_id \
                       LEFT JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
                       WHERE m.user_id = ? \
                       ORDER BY m.pinned DESC, c.last_message_at DESC, m.updated_at DESC"
            .to_string();
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params_list.iter()), |row| {
                Ok(UserWorldConversationSummaryRecord {
                    conversation_id: row.get(0)?,
                    conversation_type: row.get(1)?,
                    peer_user_id: row.get(2)?,
                    group_id: row.get(3)?,
                    group_name: row.get(4)?,
                    member_count: row.get(5)?,
                    last_read_message_id: row.get(6)?,
                    unread_count_cache: row.get(7)?,
                    pinned: row.get::<_, i64>(8)? != 0,
                    muted: row.get::<_, i64>(9)? != 0,
                    updated_at: row.get(10)?,
                    last_message_at: row.get(11)?,
                    last_message_id: row.get(12)?,
                    last_message_preview: row.get(13)?,
                })
            })?
            .collect::<std::result::Result<Vec<UserWorldConversationSummaryRecord>, _>>()?;
        Ok((rows, total))
    }

    fn list_user_world_messages_impl(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageRecord>> {
        self.ensure_initialized()?;
        let cleaned = conversation_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let safe_limit = if limit <= 0 { 50 } else { limit.min(200) };
        let mut sql = "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                       FROM user_world_messages WHERE conversation_id = ?"
            .to_string();
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned.to_string())];
        if let Some(before_id) = before_message_id.filter(|value| *value > 0) {
            sql.push_str(" AND message_id < ?");
            params_list.push(SqlValue::from(before_id));
        }
        sql.push_str(" ORDER BY message_id DESC LIMIT ?");
        params_list.push(SqlValue::from(safe_limit));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                params_from_iter(params_list.iter()),
                map_user_world_message_row,
            )?
            .collect::<std::result::Result<Vec<UserWorldMessageRecord>, _>>()?;
        Ok(rows)
    }

    fn send_user_world_message_impl(
        &self,
        conversation_id: &str,
        sender_user_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendMessageResult> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        let cleaned_sender = sender_user_id.trim();
        let cleaned_content = content.trim();
        if cleaned_conversation.is_empty()
            || cleaned_sender.is_empty()
            || cleaned_content.is_empty()
        {
            return Err(anyhow::anyhow!("invalid message payload"));
        }
        let normalized_content_type = {
            let cleaned = content_type.trim();
            if cleaned.is_empty() {
                "text"
            } else {
                cleaned
            }
        };
        let cleaned_client_msg = client_msg_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let conversation_exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM user_world_conversations WHERE conversation_id = ?",
                params![cleaned_conversation],
                |row| row.get(0),
            )
            .optional()?;
        if conversation_exists.is_none() {
            return Err(anyhow::anyhow!("conversation not found"));
        }

        let exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM user_world_members WHERE conversation_id = ? AND user_id = ?",
                params![cleaned_conversation, cleaned_sender],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(anyhow::anyhow!("sender is not a member of conversation"));
        }

        if let Some(client_msg_id) = cleaned_client_msg.as_deref() {
            if let Some(existing) = tx
                .query_row(
                    "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                     FROM user_world_messages WHERE conversation_id = ? AND client_msg_id = ?",
                    params![cleaned_conversation, client_msg_id],
                    map_user_world_message_row,
                )
                .optional()?
            {
                tx.commit()?;
                return Ok(UserWorldSendMessageResult {
                    message: existing,
                    inserted: false,
                    event: None,
                });
            }
        }

        tx.execute(
            "INSERT INTO user_world_messages (conversation_id, sender_user_id, content, content_type, client_msg_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params![
                cleaned_conversation,
                cleaned_sender,
                cleaned_content,
                normalized_content_type,
                cleaned_client_msg,
                now
            ],
        )?;
        let message_id = tx.last_insert_rowid();
        let normalized_content_type_lower = normalized_content_type.to_ascii_lowercase();
        let preview = if normalized_content_type_lower == "voice"
            || normalized_content_type_lower == "audio"
            || normalized_content_type_lower.starts_with("audio/")
            || normalized_content_type_lower.contains("voice")
        {
            "[Voice]".to_string()
        } else {
            cleaned_content.chars().take(120).collect::<String>()
        };
        tx.execute(
            "UPDATE user_world_conversations SET updated_at = ?, last_message_at = ?, last_message_id = ?, last_message_preview = ? \
             WHERE conversation_id = ?",
            params![now, now, message_id, preview, cleaned_conversation],
        )?;

        tx.execute(
            "UPDATE user_world_members SET last_read_message_id = ?, unread_count_cache = 0, updated_at = ? \
             WHERE conversation_id = ? AND user_id = ?",
            params![message_id, now, cleaned_conversation, cleaned_sender],
        )?;
        tx.execute(
            "UPDATE user_world_members SET unread_count_cache = COALESCE(unread_count_cache, 0) + 1, updated_at = ? \
             WHERE conversation_id = ? AND user_id <> ?",
            params![now, cleaned_conversation, cleaned_sender],
        )?;

        let message = tx
            .query_row(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE message_id = ?",
                params![message_id],
                map_user_world_message_row,
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("message missing after insert"))?;

        let next_event_id: i64 = tx.query_row(
            "SELECT COALESCE(MAX(event_id), 0) + 1 FROM user_world_events WHERE conversation_id = ?",
            params![cleaned_conversation],
            |row| row.get(0),
        )?;
        let payload = json!({
            "conversation_id": message.conversation_id,
            "message": {
                "message_id": message.message_id,
                "conversation_id": message.conversation_id,
                "sender_user_id": message.sender_user_id,
                "content": message.content,
                "content_type": message.content_type,
                "client_msg_id": message.client_msg_id,
                "created_at": message.created_at,
            }
        });
        tx.execute(
            "INSERT INTO user_world_events (conversation_id, event_id, event_type, payload, created_time) VALUES (?, ?, ?, ?, ?)",
            params![
                cleaned_conversation,
                next_event_id,
                "uw.message",
                Self::json_to_string(&payload),
                now
            ],
        )?;
        tx.commit()?;
        Ok(UserWorldSendMessageResult {
            message,
            inserted: true,
            event: Some(UserWorldEventRecord {
                conversation_id: cleaned_conversation.to_string(),
                event_id: next_event_id,
                event_type: "uw.message".to_string(),
                payload,
                created_time: now,
            }),
        })
    }

    fn mark_user_world_read_impl(
        &self,
        conversation_id: &str,
        user_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadResult>> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        let cleaned_user = user_id.trim();
        if cleaned_conversation.is_empty() || cleaned_user.is_empty() {
            return Ok(None);
        }
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current_member = tx
            .query_row(
                "SELECT conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, pinned, muted, updated_at \
                 FROM user_world_members WHERE conversation_id = ? AND user_id = ?",
                params![cleaned_conversation, cleaned_user],
                map_user_world_member_row,
            )
            .optional()?;
        let Some(mut member) = current_member else {
            tx.commit()?;
            return Ok(None);
        };

        let max_message_id: Option<i64> = tx.query_row(
            "SELECT MAX(message_id) FROM user_world_messages WHERE conversation_id = ?",
            params![cleaned_conversation],
            |row| row.get(0),
        )?;
        let resolved_target = match last_read_message_id.filter(|value| *value > 0) {
            Some(target) => max_message_id.map(|max_id| target.min(max_id)),
            None => max_message_id,
        };
        let current_last = member.last_read_message_id.unwrap_or(0);
        let next_last = resolved_target.unwrap_or(0).max(current_last);
        let unread_count: i64 = if next_last > 0 {
            tx.query_row(
                "SELECT COUNT(*) FROM user_world_messages \
                 WHERE conversation_id = ? AND sender_user_id <> ? AND message_id > ?",
                params![cleaned_conversation, cleaned_user, next_last],
                |row| row.get(0),
            )?
        } else {
            tx.query_row(
                "SELECT COUNT(*) FROM user_world_messages \
                 WHERE conversation_id = ? AND sender_user_id <> ?",
                params![cleaned_conversation, cleaned_user],
                |row| row.get(0),
            )?
        };

        tx.execute(
            "UPDATE user_world_members SET last_read_message_id = ?, unread_count_cache = ?, updated_at = ? \
             WHERE conversation_id = ? AND user_id = ?",
            params![
                if next_last > 0 { Some(next_last) } else { None },
                unread_count,
                now,
                cleaned_conversation,
                cleaned_user
            ],
        )?;

        let prev_last_read_message_id = member.last_read_message_id;
        let prev_unread_count = member.unread_count_cache;
        member.last_read_message_id = if next_last > 0 { Some(next_last) } else { None };
        member.unread_count_cache = unread_count;
        member.updated_at = now;

        let changed = member.last_read_message_id != prev_last_read_message_id
            || member.unread_count_cache != prev_unread_count;
        if !changed {
            tx.commit()?;
            return Ok(Some(UserWorldReadResult {
                member,
                event: None,
            }));
        }

        let next_event_id: i64 = tx.query_row(
            "SELECT COALESCE(MAX(event_id), 0) + 1 FROM user_world_events WHERE conversation_id = ?",
            params![cleaned_conversation],
            |row| row.get(0),
        )?;
        let payload = json!({
            "conversation_id": cleaned_conversation,
            "user_id": cleaned_user,
            "last_read_message_id": member.last_read_message_id,
            "unread_count": member.unread_count_cache,
        });
        tx.execute(
            "INSERT INTO user_world_events (conversation_id, event_id, event_type, payload, created_time) VALUES (?, ?, ?, ?, ?)",
            params![
                cleaned_conversation,
                next_event_id,
                "uw.read",
                Self::json_to_string(&payload),
                now
            ],
        )?;
        tx.commit()?;
        Ok(Some(UserWorldReadResult {
            member,
            event: Some(UserWorldEventRecord {
                conversation_id: cleaned_conversation.to_string(),
                event_id: next_event_id,
                event_type: "uw.read".to_string(),
                payload,
                created_time: now,
            }),
        }))
    }

    fn list_user_world_events_impl(
        &self,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldEventRecord>> {
        self.ensure_initialized()?;
        let cleaned = conversation_id.trim();
        if cleaned.is_empty() {
            return Ok(Vec::new());
        }
        let safe_limit = if limit <= 0 { 100 } else { limit.min(500) };
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT conversation_id, event_id, event_type, payload, created_time \
             FROM user_world_events WHERE conversation_id = ? AND event_id > ? \
             ORDER BY event_id ASC LIMIT ?",
        )?;
        let rows = stmt
            .query_map(params![cleaned, after_event_id.max(0), safe_limit], |row| {
                Ok(UserWorldEventRecord {
                    conversation_id: row.get(0)?,
                    event_id: row.get(1)?,
                    event_type: row.get(2)?,
                    payload: parse_json_column(row.get::<_, Option<String>>(3)?),
                    created_time: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<UserWorldEventRecord>, _>>()?;
        Ok(rows)
    }

    fn list_user_world_groups_impl(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupRecord>, i64)> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let conn = self.open()?;
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM user_world_groups g \
             JOIN user_world_members m ON m.conversation_id = g.conversation_id \
             WHERE m.user_id = ?",
            params![cleaned_user],
            |row| row.get(0),
        )?;

        let mut sql = "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
                       g.announcement, g.announcement_updated_at, \
                       (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
                       m.unread_count_cache, m.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                       FROM user_world_groups g \
                       JOIN user_world_members m ON m.conversation_id = g.conversation_id \
                       JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
                       WHERE m.user_id = ? \
                       ORDER BY m.pinned DESC, c.last_message_at DESC, g.updated_at DESC"
            .to_string();
        let mut params_list: Vec<SqlValue> = vec![SqlValue::from(cleaned_user.to_string())];
        if limit > 0 {
            sql.push_str(" LIMIT ? OFFSET ?");
            params_list.push(SqlValue::from(limit));
            params_list.push(SqlValue::from(offset.max(0)));
        }
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(
                params_from_iter(params_list.iter()),
                map_user_world_group_row,
            )?
            .collect::<std::result::Result<Vec<UserWorldGroupRecord>, _>>()?;
        Ok((rows, total))
    }

    fn get_user_world_group_by_id_impl(
        &self,
        group_id: &str,
    ) -> Result<Option<UserWorldGroupRecord>> {
        self.ensure_initialized()?;
        let cleaned_group = group_id.trim();
        if cleaned_group.is_empty() {
            return Ok(None);
        }
        let conn = self.open()?;
        conn.query_row(
            "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
             g.announcement, g.announcement_updated_at, \
             (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
             0 AS unread_count_cache, g.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
             FROM user_world_groups g \
             JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
             WHERE g.group_id = ?",
            params![cleaned_group],
            map_user_world_group_row,
        )
        .optional()
        .map_err(Into::into)
    }

    fn update_user_world_group_announcement_impl(
        &self,
        group_id: &str,
        announcement: Option<&str>,
        announcement_updated_at: Option<f64>,
        updated_at: f64,
    ) -> Result<Option<UserWorldGroupRecord>> {
        self.ensure_initialized()?;
        let cleaned_group = group_id.trim();
        if cleaned_group.is_empty() {
            return Ok(None);
        }
        let announcement = announcement
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let affected = tx.execute(
            "UPDATE user_world_groups SET announcement = ?, announcement_updated_at = ?, updated_at = ? \
             WHERE group_id = ?",
            params![
                announcement,
                announcement_updated_at,
                if updated_at.is_finite() && updated_at > 0.0 {
                    updated_at
                } else {
                    Self::now_ts()
                },
                cleaned_group
            ],
        )?;
        if affected == 0 {
            tx.commit()?;
            return Ok(None);
        }
        let record = tx
            .query_row(
                "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
                 g.announcement, g.announcement_updated_at, \
                 (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
                 0 AS unread_count_cache, g.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_groups g \
                 JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
                 WHERE g.group_id = ?",
                params![cleaned_group],
                map_user_world_group_row,
            )
            .optional()?;
        tx.commit()?;
        Ok(record)
    }

    fn list_user_world_member_user_ids_impl(&self, conversation_id: &str) -> Result<Vec<String>> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        if cleaned_conversation.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let mut stmt = conn.prepare(
            "SELECT user_id FROM user_world_members WHERE conversation_id = ? ORDER BY user_id ASC",
        )?;
        let rows = stmt.query_map(params![cleaned_conversation], |row| row.get::<_, String>(0))?;
        let mut output = Vec::new();
        for row in rows {
            let user_id = row?;
            if user_id.trim().is_empty() {
                continue;
            }
            output.push(user_id);
        }
        Ok(output)
    }

    fn list_beeroom_chat_messages_impl(
        &self,
        user_id: &str,
        group_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<BeeroomChatMessageRecord>> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_group = group_id.trim();
        if cleaned_user.is_empty() || cleaned_group.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        let safe_limit = limit.clamp(1, 200);
        let rows = if let Some(before_id) = before_message_id.filter(|value| *value > 0) {
            let mut stmt = conn.prepare(
                "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                 mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                 FROM beeroom_chat_messages WHERE user_id = ? AND group_id = ? AND message_id < ? \
                 ORDER BY message_id DESC LIMIT ?",
            )?;
            let mapped = stmt.query_map(
                params![cleaned_user, cleaned_group, before_id, safe_limit],
                map_beeroom_chat_message_row,
            )?;
            mapped.collect::<std::result::Result<Vec<BeeroomChatMessageRecord>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                 mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                 FROM beeroom_chat_messages WHERE user_id = ? AND group_id = ? \
                 ORDER BY message_id DESC LIMIT ?",
            )?;
            let mapped = stmt.query_map(
                params![cleaned_user, cleaned_group, safe_limit],
                map_beeroom_chat_message_row,
            )?;
            mapped.collect::<std::result::Result<Vec<BeeroomChatMessageRecord>, _>>()?
        };
        let mut output = rows;
        output.reverse();
        Ok(output)
    }

    fn append_beeroom_chat_message_impl(
        &self,
        user_id: &str,
        group_id: &str,
        sender_kind: &str,
        sender_name: &str,
        sender_agent_id: Option<&str>,
        mention_name: Option<&str>,
        mention_agent_id: Option<&str>,
        body: &str,
        meta: Option<&str>,
        tone: &str,
        client_msg_id: Option<&str>,
        created_at: f64,
    ) -> Result<BeeroomChatMessageRecord> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_group = group_id.trim();
        let cleaned_sender_kind = sender_kind.trim();
        let cleaned_sender_name = sender_name.trim();
        let cleaned_body = body.trim();
        if cleaned_user.is_empty()
            || cleaned_group.is_empty()
            || cleaned_sender_kind.is_empty()
            || cleaned_sender_name.is_empty()
            || cleaned_body.is_empty()
        {
            return Err(anyhow::anyhow!("invalid beeroom chat message payload"));
        }
        let cleaned_tone = if tone.trim().is_empty() {
            "system"
        } else {
            tone.trim()
        };
        let normalized_sender_agent_id = sender_agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let normalized_mention_name = mention_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let normalized_mention_agent_id = mention_agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let normalized_meta = meta
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let normalized_client_msg_id = client_msg_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let now = if created_at.is_finite() && created_at > 0.0 {
            created_at
        } else {
            Self::now_ts()
        };

        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(existing_client_id) = normalized_client_msg_id.as_deref() {
            if let Some(existing) = tx
                .query_row(
                    "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                     mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                     FROM beeroom_chat_messages WHERE user_id = ? AND group_id = ? AND client_msg_id = ?",
                    params![cleaned_user, cleaned_group, existing_client_id],
                    map_beeroom_chat_message_row,
                )
                .optional()?
            {
                tx.commit()?;
                return Ok(existing);
            }
        }
        tx.execute(
            "INSERT INTO beeroom_chat_messages \
             (user_id, group_id, sender_kind, sender_name, sender_agent_id, mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                cleaned_user,
                cleaned_group,
                cleaned_sender_kind,
                cleaned_sender_name,
                normalized_sender_agent_id,
                normalized_mention_name,
                normalized_mention_agent_id,
                cleaned_body,
                normalized_meta,
                cleaned_tone,
                normalized_client_msg_id,
                now
            ],
        )?;
        let message_id = tx.last_insert_rowid();
        let record = tx
            .query_row(
                "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                 mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                 FROM beeroom_chat_messages WHERE message_id = ?",
                params![message_id],
                map_beeroom_chat_message_row,
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("beeroom chat message missing after insert"))?;
        tx.commit()?;
        Ok(record)
    }

    fn delete_beeroom_chat_messages_impl(&self, user_id: &str, group_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_group = group_id.trim();
        if cleaned_user.is_empty() || cleaned_group.is_empty() {
            return Ok(0);
        }
        let conn = self.open()?;
        conn.execute(
            "DELETE FROM beeroom_chat_messages WHERE user_id = ? AND group_id = ?",
            params![cleaned_user, cleaned_group],
        )
        .map(|count| count as i64)
        .map_err(Into::into)
    }
}

fn normalize_user_world_pair(user_a: &str, user_b: &str) -> Option<(String, String)> {
    let a = user_a.trim();
    let b = user_b.trim();
    if a.is_empty() || b.is_empty() || a == b {
        return None;
    }
    if a <= b {
        Some((a.to_string(), b.to_string()))
    } else {
        Some((b.to_string(), a.to_string()))
    }
}

fn normalize_user_world_members(owner_user_id: &str, member_user_ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let owner = owner_user_id.trim();
    if !owner.is_empty() {
        seen.insert(owner.to_string());
        output.push(owner.to_string());
    }
    for raw in member_user_ids {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            output.push(cleaned.to_string());
        }
    }
    output
}

fn parse_json_column(value: Option<String>) -> Value {
    value
        .as_deref()
        .and_then(SqliteStorage::json_from_str)
        .unwrap_or(Value::Null)
}

fn map_user_world_conversation_row(row: &Row<'_>) -> rusqlite::Result<UserWorldConversationRecord> {
    Ok(UserWorldConversationRecord {
        conversation_id: row.get(0)?,
        conversation_type: row.get(1)?,
        participant_a: row.get(2)?,
        participant_b: row.get(3)?,
        group_id: row.get(4)?,
        group_name: row.get(5)?,
        member_count: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        last_message_at: row.get(9)?,
        last_message_id: row.get(10)?,
        last_message_preview: row.get(11)?,
    })
}

fn map_user_world_group_row(row: &Row<'_>) -> rusqlite::Result<UserWorldGroupRecord> {
    Ok(UserWorldGroupRecord {
        group_id: row.get(0)?,
        conversation_id: row.get(1)?,
        group_name: row.get(2)?,
        owner_user_id: row.get(3)?,
        announcement: row.get(4)?,
        announcement_updated_at: row.get(5)?,
        member_count: row.get(6)?,
        unread_count_cache: row.get(7)?,
        updated_at: row.get(8)?,
        last_message_at: row.get(9)?,
        last_message_id: row.get(10)?,
        last_message_preview: row.get(11)?,
    })
}

fn map_user_world_member_row(row: &Row<'_>) -> rusqlite::Result<UserWorldMemberRecord> {
    let pinned: Option<i64> = row.get(5)?;
    let muted: Option<i64> = row.get(6)?;
    Ok(UserWorldMemberRecord {
        conversation_id: row.get(0)?,
        user_id: row.get(1)?,
        peer_user_id: row.get(2)?,
        last_read_message_id: row.get(3)?,
        unread_count_cache: row.get(4)?,
        pinned: pinned.unwrap_or(0) != 0,
        muted: muted.unwrap_or(0) != 0,
        updated_at: row.get(7)?,
    })
}

fn map_user_world_message_row(row: &Row<'_>) -> rusqlite::Result<UserWorldMessageRecord> {
    Ok(UserWorldMessageRecord {
        message_id: row.get(0)?,
        conversation_id: row.get(1)?,
        sender_user_id: row.get(2)?,
        content: row.get(3)?,
        content_type: row.get(4)?,
        client_msg_id: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn map_beeroom_chat_message_row(row: &Row<'_>) -> rusqlite::Result<BeeroomChatMessageRecord> {
    Ok(BeeroomChatMessageRecord {
        message_id: row.get(0)?,
        user_id: row.get(1)?,
        group_id: row.get(2)?,
        sender_kind: row.get(3)?,
        sender_name: row.get(4)?,
        sender_agent_id: row.get(5)?,
        mention_name: row.get(6)?,
        mention_agent_id: row.get(7)?,
        body: row.get(8)?,
        meta: row.get(9)?,
        tone: row.get(10)?,
        client_msg_id: row.get(11)?,
        created_at: row.get(12)?,
    })
}
