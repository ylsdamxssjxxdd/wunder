use super::PostgresStorage;
use crate::storage::{
    BeeroomChatMessageRecord, StorageLifecycle, UserWorldConversationRecord,
    UserWorldConversationSummaryRecord, UserWorldEventRecord, UserWorldGroupRecord,
    UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult, UserWorldSendMessageResult,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use tokio_postgres::Row;

pub(super) trait PostgresUserWorldStorage {
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

impl PostgresUserWorldStorage for PostgresStorage {
    fn resolve_or_create_user_world_direct_conversation_impl(
        &self,
        user_a: &str,
        user_b: &str,
        now: f64,
    ) -> Result<UserWorldConversationRecord> {
        self.ensure_initialized()?;
        let (participant_a, participant_b) = normalize_user_world_pair(user_a, user_b)
            .ok_or_else(|| anyhow!("invalid user pair"))?;
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let existing = tx.query_opt(
            "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
             NULL::TEXT AS group_id, NULL::TEXT AS group_name, 2::BIGINT AS member_count, \
             c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
             FROM user_world_conversations c \
             WHERE c.conversation_type = 'direct' AND c.participant_a = $1 AND c.participant_b = $2",
            &[&participant_a, &participant_b],
        )?;
        if let Some(row) = existing {
            let record = map_user_world_conversation_row(&row);
            tx.commit()?;
            return Ok(record);
        }
        let conversation_id = format!("uwc_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO user_world_conversations (conversation_id, conversation_type, participant_a, participant_b, \
             created_at, updated_at, last_message_at, last_message_id, last_message_preview) \
             VALUES ($1, 'direct', $2, $3, $4, $5, $6, NULL, NULL)",
            &[&conversation_id, &participant_a, &participant_b, &now, &now, &now],
        )?;
        tx.execute(
            "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
             pinned, muted, updated_at) VALUES ($1, $2, $3, NULL, 0, 0, 0, $4)",
            &[&conversation_id, &participant_a, &participant_b, &now],
        )?;
        tx.execute(
            "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
             pinned, muted, updated_at) VALUES ($1, $2, $3, NULL, 0, 0, 0, $4)",
            &[&conversation_id, &participant_b, &participant_a, &now],
        )?;
        let row = tx
            .query_opt(
                "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
                 NULL::TEXT AS group_id, NULL::TEXT AS group_name, 2::BIGINT AS member_count, \
                 c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_conversations c WHERE c.conversation_id = $1",
                &[&conversation_id],
            )?
            .ok_or_else(|| anyhow!("user world conversation missing after insert"))?;
        let record = map_user_world_conversation_row(&row);
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
            return Err(anyhow!("owner_user_id/group_name is required"));
        }
        let members = normalize_user_world_members(owner, member_user_ids);
        if members.len() < 2 {
            return Err(anyhow!("group requires at least 2 users"));
        }
        let now = if now.is_finite() && now > 0.0 {
            now
        } else {
            Self::now_ts()
        };
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let conversation_id = format!("uwc_{}", uuid::Uuid::new_v4().simple());
        let group_id = format!("uwg_{}", uuid::Uuid::new_v4().simple());
        tx.execute(
            "INSERT INTO user_world_conversations (conversation_id, conversation_type, participant_a, participant_b, \
             created_at, updated_at, last_message_at, last_message_id, last_message_preview) \
             VALUES ($1, 'group', $2, $3, $4, $5, $6, NULL, NULL)",
            &[&conversation_id, &owner, &group_id, &now, &now, &now],
        )?;
        tx.execute(
            "INSERT INTO user_world_groups (group_id, conversation_id, group_name, owner_user_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&group_id, &conversation_id, &name, &owner, &now, &now],
        )?;
        for member_user_id in &members {
            tx.execute(
                "INSERT INTO user_world_members (conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, \
                 pinned, muted, updated_at) VALUES ($1, $2, '', NULL, 0, 0, 0, $3)",
                &[&conversation_id, member_user_id, &now],
            )?;
        }
        let member_count = members.len() as i64;
        let row = tx
            .query_opt(
                "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
                 g.group_id, g.group_name, $1::BIGINT AS member_count, \
                 c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_conversations c \
                 JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
                 WHERE c.conversation_id = $2",
                &[&member_count, &conversation_id],
            )?
            .ok_or_else(|| anyhow!("user world group missing after insert"))?;
        let record = map_user_world_conversation_row(&row);
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT c.conversation_id, c.conversation_type, c.participant_a, c.participant_b, \
             g.group_id, g.group_name, \
             CASE WHEN c.conversation_type = 'group' THEN \
                (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = c.conversation_id) \
             ELSE NULL END AS member_count, \
             c.created_at, c.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
             FROM user_world_conversations c \
             LEFT JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
             WHERE c.conversation_id = $1",
            &[&cleaned],
        )?;
        Ok(row.map(|row| map_user_world_conversation_row(&row)))
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, pinned, muted, updated_at \
             FROM user_world_members WHERE conversation_id = $1 AND user_id = $2",
            &[&cleaned_conversation, &cleaned_user],
        )?;
        Ok(row.map(|row| map_user_world_member_row(&row)))
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
        let mut conn = self.conn()?;
        let total_row = conn.query_one(
            "SELECT COUNT(*) FROM user_world_members WHERE user_id = $1",
            &[&cleaned_user],
        )?;
        let total: i64 = total_row.get(0);
        let rows = if limit > 0 {
            let safe_limit = limit.max(1);
            let safe_offset = offset.max(0);
            conn.query(
                "SELECT c.conversation_id, c.conversation_type, m.peer_user_id, \
                 g.group_id, g.group_name, \
                 CASE WHEN c.conversation_type = 'group' THEN \
                    (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = c.conversation_id) \
                 ELSE NULL END AS member_count, \
                 m.last_read_message_id, m.unread_count_cache, m.pinned, m.muted, m.updated_at, \
                 c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_members m \
                 JOIN user_world_conversations c ON c.conversation_id = m.conversation_id \
                 LEFT JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
                 WHERE m.user_id = $1 \
                 ORDER BY m.pinned DESC, c.last_message_at DESC, m.updated_at DESC \
                 LIMIT $2 OFFSET $3",
                &[&cleaned_user, &safe_limit, &safe_offset],
            )?
        } else {
            conn.query(
                "SELECT c.conversation_id, c.conversation_type, m.peer_user_id, \
                 g.group_id, g.group_name, \
                 CASE WHEN c.conversation_type = 'group' THEN \
                    (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = c.conversation_id) \
                 ELSE NULL END AS member_count, \
                 m.last_read_message_id, m.unread_count_cache, m.pinned, m.muted, m.updated_at, \
                 c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_members m \
                 JOIN user_world_conversations c ON c.conversation_id = m.conversation_id \
                 LEFT JOIN user_world_groups g ON g.conversation_id = c.conversation_id \
                 WHERE m.user_id = $1 \
                 ORDER BY m.pinned DESC, c.last_message_at DESC, m.updated_at DESC",
                &[&cleaned_user],
            )?
        };
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(UserWorldConversationSummaryRecord {
                conversation_id: row.get(0),
                conversation_type: row.get(1),
                peer_user_id: row.get(2),
                group_id: row.get(3),
                group_name: row.get(4),
                member_count: row.get(5),
                last_read_message_id: row.get(6),
                unread_count_cache: row.get(7),
                pinned: row.get::<_, i32>(8) != 0,
                muted: row.get::<_, i32>(9) != 0,
                updated_at: row.get(10),
                last_message_at: row.get(11),
                last_message_id: row.get(12),
                last_message_preview: row.get(13),
            });
        }
        Ok((output, total))
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
        let safe_limit = if limit <= 0 { 50 } else { limit.min(200) };
        let mut conn = self.conn()?;
        let rows = if let Some(before_id) = before_message_id.filter(|value| *value > 0) {
            conn.query(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE conversation_id = $1 AND message_id < $2 \
                 ORDER BY message_id DESC LIMIT $3",
                &[&cleaned, &before_id, &safe_limit],
            )?
        } else {
            conn.query(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE conversation_id = $1 \
                 ORDER BY message_id DESC LIMIT $2",
                &[&cleaned, &safe_limit],
            )?
        };
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(map_user_world_message_row(&row));
        }
        Ok(output)
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
            return Err(anyhow!("invalid message payload"));
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let conversation_exists = tx.query_opt(
            "SELECT 1 FROM user_world_conversations WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        if conversation_exists.is_none() {
            return Err(anyhow!("conversation not found"));
        }
        let member_exists = tx.query_opt(
            "SELECT 1 FROM user_world_members WHERE conversation_id = $1 AND user_id = $2",
            &[&cleaned_conversation, &cleaned_sender],
        )?;
        if member_exists.is_none() {
            return Err(anyhow!("sender is not a member of conversation"));
        }

        if let Some(client_msg_id) = cleaned_client_msg.as_deref() {
            if let Some(existing) = tx.query_opt(
                "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
                 FROM user_world_messages WHERE conversation_id = $1 AND client_msg_id = $2",
                &[&cleaned_conversation, &client_msg_id],
            )? {
                let message = map_user_world_message_row(&existing);
                tx.commit()?;
                return Ok(UserWorldSendMessageResult {
                    message,
                    inserted: false,
                    event: None,
                });
            }
        }

        let insert_row = tx.query_one(
            "INSERT INTO user_world_messages (conversation_id, sender_user_id, content, content_type, client_msg_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING message_id",
            &[
                &cleaned_conversation,
                &cleaned_sender,
                &cleaned_content,
                &normalized_content_type,
                &cleaned_client_msg,
                &now,
            ],
        )?;
        let message_id: i64 = insert_row.get(0);
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
            "UPDATE user_world_conversations SET updated_at = $1, last_message_at = $2, last_message_id = $3, \
             last_message_preview = $4 WHERE conversation_id = $5",
            &[&now, &now, &message_id, &preview, &cleaned_conversation],
        )?;
        tx.execute(
            "UPDATE user_world_members SET last_read_message_id = $1, unread_count_cache = 0, updated_at = $2 \
             WHERE conversation_id = $3 AND user_id = $4",
            &[&message_id, &now, &cleaned_conversation, &cleaned_sender],
        )?;
        tx.execute(
            "UPDATE user_world_members SET unread_count_cache = COALESCE(unread_count_cache, 0) + 1, updated_at = $1 \
             WHERE conversation_id = $2 AND user_id <> $3",
            &[&now, &cleaned_conversation, &cleaned_sender],
        )?;

        let message_row = tx.query_one(
            "SELECT message_id, conversation_id, sender_user_id, content, content_type, client_msg_id, created_at \
             FROM user_world_messages WHERE message_id = $1",
            &[&message_id],
        )?;
        let message = map_user_world_message_row(&message_row);
        let event_id_row = tx.query_one(
            "SELECT COALESCE(MAX(event_id), 0) + 1 FROM user_world_events WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        let next_event_id: i64 = event_id_row.get(0);
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
        let payload_text = Self::json_to_string(&payload);
        tx.execute(
            "INSERT INTO user_world_events (conversation_id, event_id, event_type, payload, created_time) VALUES ($1, $2, $3, $4, $5)",
            &[&cleaned_conversation, &next_event_id, &"uw.message", &payload_text, &now],
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
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let current_member_row = tx.query_opt(
            "SELECT conversation_id, user_id, peer_user_id, last_read_message_id, unread_count_cache, pinned, muted, updated_at \
             FROM user_world_members WHERE conversation_id = $1 AND user_id = $2",
            &[&cleaned_conversation, &cleaned_user],
        )?;
        let Some(current_member_row) = current_member_row else {
            tx.commit()?;
            return Ok(None);
        };
        let mut member = map_user_world_member_row(&current_member_row);
        let prev_last_read_message_id = member.last_read_message_id;
        let prev_unread_count = member.unread_count_cache;

        let max_message_row = tx.query_one(
            "SELECT MAX(message_id) FROM user_world_messages WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        let max_message_id: Option<i64> = max_message_row.get(0);
        let resolved_target = match last_read_message_id.filter(|value| *value > 0) {
            Some(target) => max_message_id.map(|max_id| target.min(max_id)),
            None => max_message_id,
        };
        let current_last = member.last_read_message_id.unwrap_or(0);
        let next_last = resolved_target.unwrap_or(0).max(current_last);
        let unread_query = if next_last > 0 {
            tx.query_one(
                "SELECT COUNT(*) FROM user_world_messages \
                 WHERE conversation_id = $1 AND sender_user_id <> $2 AND message_id > $3",
                &[&cleaned_conversation, &cleaned_user, &next_last],
            )?
        } else {
            tx.query_one(
                "SELECT COUNT(*) FROM user_world_messages \
                 WHERE conversation_id = $1 AND sender_user_id <> $2",
                &[&cleaned_conversation, &cleaned_user],
            )?
        };
        let unread_count: i64 = unread_query.get(0);
        let next_last_opt = if next_last > 0 { Some(next_last) } else { None };
        tx.execute(
            "UPDATE user_world_members SET last_read_message_id = $1, unread_count_cache = $2, updated_at = $3 \
             WHERE conversation_id = $4 AND user_id = $5",
            &[
                &next_last_opt,
                &unread_count,
                &now,
                &cleaned_conversation,
                &cleaned_user,
            ],
        )?;
        member.last_read_message_id = next_last_opt;
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

        let next_event_id_row = tx.query_one(
            "SELECT COALESCE(MAX(event_id), 0) + 1 FROM user_world_events WHERE conversation_id = $1",
            &[&cleaned_conversation],
        )?;
        let next_event_id: i64 = next_event_id_row.get(0);
        let payload = json!({
            "conversation_id": cleaned_conversation,
            "user_id": cleaned_user,
            "last_read_message_id": member.last_read_message_id,
            "unread_count": member.unread_count_cache,
        });
        let payload_text = Self::json_to_string(&payload);
        tx.execute(
            "INSERT INTO user_world_events (conversation_id, event_id, event_type, payload, created_time) VALUES ($1, $2, $3, $4, $5)",
            &[&cleaned_conversation, &next_event_id, &"uw.read", &payload_text, &now],
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
        let safe_after = after_event_id.max(0);
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT conversation_id, event_id, event_type, payload, created_time \
             FROM user_world_events WHERE conversation_id = $1 AND event_id > $2 \
             ORDER BY event_id ASC LIMIT $3",
            &[&cleaned, &safe_after, &safe_limit],
        )?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let payload_text: Option<String> = row.get(3);
            output.push(UserWorldEventRecord {
                conversation_id: row.get(0),
                event_id: row.get(1),
                event_type: row.get(2),
                payload: parse_json_column(payload_text),
                created_time: row.get(4),
            });
        }
        Ok(output)
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
        let mut conn = self.conn()?;
        let total_row = conn.query_one(
            "SELECT COUNT(*) FROM user_world_groups g \
             JOIN user_world_members m ON m.conversation_id = g.conversation_id \
             WHERE m.user_id = $1",
            &[&cleaned_user],
        )?;
        let total: i64 = total_row.get(0);
        let rows = if limit > 0 {
            let safe_limit = limit.max(1);
            let safe_offset = offset.max(0);
            conn.query(
                "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
                 g.announcement, g.announcement_updated_at, \
                 (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
                 m.unread_count_cache, m.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_groups g \
                 JOIN user_world_members m ON m.conversation_id = g.conversation_id \
                 JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
                 WHERE m.user_id = $1 \
                 ORDER BY m.pinned DESC, c.last_message_at DESC, g.updated_at DESC \
                 LIMIT $2 OFFSET $3",
                &[&cleaned_user, &safe_limit, &safe_offset],
            )?
        } else {
            conn.query(
                "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
                 g.announcement, g.announcement_updated_at, \
                 (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
                 m.unread_count_cache, m.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
                 FROM user_world_groups g \
                 JOIN user_world_members m ON m.conversation_id = g.conversation_id \
                 JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
                 WHERE m.user_id = $1 \
                 ORDER BY m.pinned DESC, c.last_message_at DESC, g.updated_at DESC",
                &[&cleaned_user],
            )?
        };
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            output.push(map_user_world_group_row(&row));
        }
        Ok((output, total))
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
        let mut conn = self.conn()?;
        let row = conn.query_opt(
            "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
             g.announcement, g.announcement_updated_at, \
             (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
             0::BIGINT AS unread_count_cache, g.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
             FROM user_world_groups g \
             JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
             WHERE g.group_id = $1",
            &[&cleaned_group],
        )?;
        Ok(row.map(|row| map_user_world_group_row(&row)))
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
        let safe_updated_at = if updated_at.is_finite() && updated_at > 0.0 {
            updated_at
        } else {
            Self::now_ts()
        };
        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        let affected = tx.execute(
            "UPDATE user_world_groups SET announcement = $1, announcement_updated_at = $2, updated_at = $3 \
             WHERE group_id = $4",
            &[
                &announcement,
                &announcement_updated_at,
                &safe_updated_at,
                &cleaned_group,
            ],
        )?;
        if affected == 0 {
            tx.commit()?;
            return Ok(None);
        }
        let row = tx.query_opt(
            "SELECT g.group_id, g.conversation_id, g.group_name, g.owner_user_id, \
             g.announcement, g.announcement_updated_at, \
             (SELECT COUNT(*) FROM user_world_members mm WHERE mm.conversation_id = g.conversation_id) AS member_count, \
             0::BIGINT AS unread_count_cache, g.updated_at, c.last_message_at, c.last_message_id, c.last_message_preview \
             FROM user_world_groups g \
             JOIN user_world_conversations c ON c.conversation_id = g.conversation_id \
             WHERE g.group_id = $1",
            &[&cleaned_group],
        )?;
        tx.commit()?;
        Ok(row.map(|row| map_user_world_group_row(&row)))
    }

    fn list_user_world_member_user_ids_impl(&self, conversation_id: &str) -> Result<Vec<String>> {
        self.ensure_initialized()?;
        let cleaned_conversation = conversation_id.trim();
        if cleaned_conversation.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.conn()?;
        let rows = conn.query(
            "SELECT user_id FROM user_world_members WHERE conversation_id = $1 ORDER BY user_id ASC",
            &[&cleaned_conversation],
        )?;
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let user_id: String = row.get(0);
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
        let safe_limit = limit.clamp(1, 200);
        let mut conn = self.conn()?;
        let rows = if let Some(before_id) = before_message_id.filter(|value| *value > 0) {
            conn.query(
                "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                 mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                 FROM beeroom_chat_messages WHERE user_id = $1 AND group_id = $2 AND message_id < $3 \
                 ORDER BY message_id DESC LIMIT $4",
                &[&cleaned_user, &cleaned_group, &before_id, &safe_limit],
            )?
        } else {
            conn.query(
                "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                 mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                 FROM beeroom_chat_messages WHERE user_id = $1 AND group_id = $2 \
                 ORDER BY message_id DESC LIMIT $3",
                &[&cleaned_user, &cleaned_group, &safe_limit],
            )?
        };
        let mut output = rows
            .into_iter()
            .map(|row| map_beeroom_chat_message_row(&row))
            .collect::<Vec<_>>();
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
            return Err(anyhow!("invalid beeroom chat message payload"));
        }
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
        let normalized_tone = if tone.trim().is_empty() {
            "system".to_string()
        } else {
            tone.trim().to_string()
        };
        let normalized_client_msg_id = client_msg_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let safe_created_at = if created_at.is_finite() && created_at > 0.0 {
            created_at
        } else {
            Self::now_ts()
        };

        let mut conn = self.conn()?;
        let mut tx = conn.transaction()?;
        if let Some(existing_client_msg_id) = normalized_client_msg_id.as_deref() {
            if let Some(existing) = tx.query_opt(
                "SELECT message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, \
                 mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at \
                 FROM beeroom_chat_messages WHERE user_id = $1 AND group_id = $2 AND client_msg_id = $3",
                &[&cleaned_user, &cleaned_group, &existing_client_msg_id],
            )? {
                tx.commit()?;
                return Ok(map_beeroom_chat_message_row(&existing));
            }
        }
        let inserted = tx.query_one(
            "INSERT INTO beeroom_chat_messages \
             (user_id, group_id, sender_kind, sender_name, sender_agent_id, mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
             RETURNING message_id, user_id, group_id, sender_kind, sender_name, sender_agent_id, mention_name, mention_agent_id, body, meta, tone, client_msg_id, created_at",
            &[
                &cleaned_user,
                &cleaned_group,
                &cleaned_sender_kind,
                &cleaned_sender_name,
                &normalized_sender_agent_id,
                &normalized_mention_name,
                &normalized_mention_agent_id,
                &cleaned_body,
                &normalized_meta,
                &normalized_tone,
                &normalized_client_msg_id,
                &safe_created_at,
            ],
        )?;
        tx.commit()?;
        Ok(map_beeroom_chat_message_row(&inserted))
    }

    fn delete_beeroom_chat_messages_impl(&self, user_id: &str, group_id: &str) -> Result<i64> {
        self.ensure_initialized()?;
        let cleaned_user = user_id.trim();
        let cleaned_group = group_id.trim();
        if cleaned_user.is_empty() || cleaned_group.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn()?;
        let deleted = conn.execute(
            "DELETE FROM beeroom_chat_messages WHERE user_id = $1 AND group_id = $2",
            &[&cleaned_user, &cleaned_group],
        )?;
        Ok(deleted as i64)
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
        .and_then(PostgresStorage::json_from_str)
        .unwrap_or(Value::Null)
}

fn map_user_world_conversation_row(row: &Row) -> UserWorldConversationRecord {
    UserWorldConversationRecord {
        conversation_id: row.get(0),
        conversation_type: row.get(1),
        participant_a: row.get(2),
        participant_b: row.get(3),
        group_id: row.get(4),
        group_name: row.get(5),
        member_count: row.get(6),
        created_at: row.get(7),
        updated_at: row.get(8),
        last_message_at: row.get(9),
        last_message_id: row.get(10),
        last_message_preview: row.get(11),
    }
}

fn map_user_world_group_row(row: &Row) -> UserWorldGroupRecord {
    UserWorldGroupRecord {
        group_id: row.get(0),
        conversation_id: row.get(1),
        group_name: row.get(2),
        owner_user_id: row.get(3),
        announcement: row.get(4),
        announcement_updated_at: row.get(5),
        member_count: row.get(6),
        unread_count_cache: row.get(7),
        updated_at: row.get(8),
        last_message_at: row.get(9),
        last_message_id: row.get(10),
        last_message_preview: row.get(11),
    }
}

fn map_user_world_member_row(row: &Row) -> UserWorldMemberRecord {
    UserWorldMemberRecord {
        conversation_id: row.get(0),
        user_id: row.get(1),
        peer_user_id: row.get(2),
        last_read_message_id: row.get(3),
        unread_count_cache: row.get(4),
        pinned: row.get::<_, i32>(5) != 0,
        muted: row.get::<_, i32>(6) != 0,
        updated_at: row.get(7),
    }
}

fn map_user_world_message_row(row: &Row) -> UserWorldMessageRecord {
    UserWorldMessageRecord {
        message_id: row.get(0),
        conversation_id: row.get(1),
        sender_user_id: row.get(2),
        content: row.get(3),
        content_type: row.get(4),
        client_msg_id: row.get(5),
        created_at: row.get(6),
    }
}

fn map_beeroom_chat_message_row(row: &Row) -> BeeroomChatMessageRecord {
    BeeroomChatMessageRecord {
        message_id: row.get(0),
        user_id: row.get(1),
        group_id: row.get(2),
        sender_kind: row.get(3),
        sender_name: row.get(4),
        sender_agent_id: row.get(5),
        mention_name: row.get(6),
        mention_agent_id: row.get(7),
        body: row.get(8),
        meta: row.get(9),
        tone: row.get(10),
        client_msg_id: row.get(11),
        created_at: row.get(12),
    }
}
