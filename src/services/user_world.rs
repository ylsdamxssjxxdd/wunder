use crate::storage::{
    StorageBackend, UserAccountRecord, UserWorldConversationRecord,
    UserWorldConversationSummaryRecord, UserWorldEventRecord, UserWorldGroupRecord,
    UserWorldMemberRecord, UserWorldMessageRecord, UserWorldReadResult,
};
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

const DEFAULT_CONTACT_LIMIT: i64 = 100;
const MAX_CONTACT_FETCH: i64 = 10_000;
const DEFAULT_LIST_LIMIT: i64 = 50;
const MAX_GROUP_ANNOUNCEMENT_LEN: usize = 4_000;

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldContact {
    pub user_id: String,
    pub username: String,
    pub status: String,
    pub online: bool,
    pub last_seen_at: Option<f64>,
    pub unit_id: Option<String>,
    pub conversation_id: Option<String>,
    pub last_message_preview: Option<String>,
    pub last_message_at: Option<f64>,
    pub unread_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldConversationView {
    pub conversation_id: String,
    pub conversation_type: String,
    pub peer_user_id: String,
    pub group_id: Option<String>,
    pub group_name: Option<String>,
    pub member_count: Option<i64>,
    pub last_read_message_id: Option<i64>,
    pub unread_count_cache: i64,
    pub pinned: bool,
    pub muted: bool,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub last_message_id: Option<i64>,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldGroupView {
    pub group_id: String,
    pub conversation_id: String,
    pub group_name: String,
    pub owner_user_id: String,
    pub announcement: Option<String>,
    pub announcement_updated_at: Option<f64>,
    pub member_count: i64,
    pub unread_count_cache: i64,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub last_message_id: Option<i64>,
    pub last_message_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldGroupMemberView {
    pub user_id: String,
    pub username: String,
    pub status: String,
    pub unit_id: Option<String>,
    pub is_owner: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldGroupDetailView {
    pub group_id: String,
    pub conversation_id: String,
    pub group_name: String,
    pub owner_user_id: String,
    pub announcement: Option<String>,
    pub announcement_updated_at: Option<f64>,
    pub member_count: i64,
    pub updated_at: f64,
    pub last_message_at: f64,
    pub last_message_id: Option<i64>,
    pub last_message_preview: Option<String>,
    pub members: Vec<UserWorldGroupMemberView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldMessageView {
    pub message_id: i64,
    pub conversation_id: String,
    pub sender_user_id: String,
    pub content: String,
    pub content_type: String,
    pub client_msg_id: Option<String>,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldRealtimeEvent {
    pub conversation_id: String,
    pub event_id: i64,
    pub event_type: String,
    pub payload: Value,
    pub created_time: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldReadView {
    pub conversation_id: String,
    pub user_id: String,
    pub peer_user_id: String,
    pub last_read_message_id: Option<i64>,
    pub unread_count_cache: i64,
    pub updated_at: f64,
    pub event: Option<UserWorldRealtimeEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserWorldSendView {
    pub message: UserWorldMessageView,
    pub inserted: bool,
    pub event: Option<UserWorldRealtimeEvent>,
}

pub struct UserWorldService {
    storage: Arc<dyn StorageBackend>,
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<UserWorldRealtimeEvent>>>>,
}

impl UserWorldService {
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self {
            storage,
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn list_contacts(
        &self,
        user_id: &str,
        keyword: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldContact>, i64)> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let fetch_limit =
            (offset.max(0) + limit.max(DEFAULT_CONTACT_LIMIT) + 200).min(MAX_CONTACT_FETCH);
        let (users, _) = self
            .storage
            .list_user_accounts(keyword, None, 0, fetch_limit)?;
        let (conversations, _) =
            self.storage
                .list_user_world_conversations(cleaned_user, 0, MAX_CONTACT_FETCH)?;
        let mut conversation_map = HashMap::new();
        for item in conversations {
            if !item.conversation_type.eq_ignore_ascii_case("direct")
                || item.peer_user_id.trim().is_empty()
            {
                continue;
            }
            conversation_map.insert(item.peer_user_id.clone(), item);
        }
        let mut contacts = users
            .into_iter()
            .filter(|item| item.user_id != cleaned_user)
            .filter(|item| item.status.trim().eq_ignore_ascii_case("active"))
            .map(|item| {
                let summary = conversation_map.get(&item.user_id);
                self.map_contact(&item, summary)
            })
            .collect::<Vec<_>>();
        contacts.sort_by(|left, right| {
            let left_ts = left.last_message_at.unwrap_or(0.0);
            let right_ts = right.last_message_at.unwrap_or(0.0);
            right_ts
                .total_cmp(&left_ts)
                .then_with(|| left.username.cmp(&right.username))
        });
        let total = contacts.len() as i64;
        let start = offset.max(0) as usize;
        let end = if limit <= 0 {
            contacts.len()
        } else {
            start.saturating_add(limit as usize).min(contacts.len())
        };
        if start >= contacts.len() {
            return Ok((Vec::new(), total));
        }
        Ok((contacts[start..end].to_vec(), total))
    }

    pub fn resolve_or_create_direct_conversation(
        &self,
        user_id: &str,
        peer_user_id: &str,
        now: f64,
    ) -> Result<UserWorldConversationView> {
        let cleaned_user = user_id.trim();
        let cleaned_peer = peer_user_id.trim();
        if cleaned_user.is_empty() || cleaned_peer.is_empty() {
            return Err(anyhow!("user_id or peer_user_id is empty"));
        }
        if cleaned_user == cleaned_peer {
            return Err(anyhow!("cannot create conversation with self"));
        }
        let conversation = self
            .storage
            .resolve_or_create_user_world_direct_conversation(cleaned_user, cleaned_peer, now)?;
        let member = self
            .storage
            .get_user_world_member(&conversation.conversation_id, cleaned_user)?
            .ok_or_else(|| anyhow!("conversation member missing"))?;
        Ok(Self::map_conversation_view(&conversation, &member))
    }

    pub fn create_group(
        &self,
        owner_user_id: &str,
        group_name: &str,
        member_user_ids: &[String],
        now: f64,
    ) -> Result<UserWorldConversationView> {
        let owner = owner_user_id.trim();
        if owner.is_empty() {
            return Err(anyhow!("owner_user_id is empty"));
        }
        let cleaned_name = group_name.trim();
        if cleaned_name.is_empty() {
            return Err(anyhow!("group_name is required"));
        }
        let conversation =
            self.storage
                .create_user_world_group(owner, cleaned_name, member_user_ids, now)?;
        let member = self
            .storage
            .get_user_world_member(&conversation.conversation_id, owner)?
            .ok_or_else(|| anyhow!("conversation member missing"))?;
        Ok(Self::map_conversation_view(&conversation, &member))
    }

    pub fn list_groups(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldGroupView>, i64)> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let (items, total) = self.storage.list_user_world_groups(
            cleaned_user,
            offset.max(0),
            normalize_limit(limit, DEFAULT_LIST_LIMIT, 500),
        )?;
        let output = items.iter().map(Self::map_group).collect::<Vec<_>>();
        Ok((output, total))
    }

    pub fn get_group_detail(
        &self,
        user_id: &str,
        group_id: &str,
    ) -> Result<Option<UserWorldGroupDetailView>> {
        let cleaned_user = user_id.trim();
        let cleaned_group = group_id.trim();
        if cleaned_user.is_empty() || cleaned_group.is_empty() {
            return Ok(None);
        }
        let Some(group) = self.storage.get_user_world_group_by_id(cleaned_group)? else {
            return Ok(None);
        };
        if self
            .storage
            .get_user_world_member(&group.conversation_id, cleaned_user)?
            .is_none()
        {
            return Ok(None);
        }
        self.build_group_detail(&group).map(Some)
    }

    pub fn update_group_announcement(
        &self,
        user_id: &str,
        group_id: &str,
        announcement: Option<&str>,
        now: f64,
    ) -> Result<Option<UserWorldGroupDetailView>> {
        let cleaned_user = user_id.trim();
        let cleaned_group = group_id.trim();
        if cleaned_user.is_empty() || cleaned_group.is_empty() {
            return Ok(None);
        }
        let Some(current_group) = self.storage.get_user_world_group_by_id(cleaned_group)? else {
            return Ok(None);
        };
        self.ensure_member(cleaned_user, &current_group.conversation_id)?;
        let normalized_announcement = announcement
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(value) = &normalized_announcement {
            if value.chars().count() > MAX_GROUP_ANNOUNCEMENT_LEN {
                return Err(anyhow!(
                    "announcement is too long (max {MAX_GROUP_ANNOUNCEMENT_LEN} characters)"
                ));
            }
        }
        let safe_now = if now.is_finite() && now > 0.0 {
            now
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_secs_f64())
                .unwrap_or(0.0)
        };
        let updated = self.storage.update_user_world_group_announcement(
            cleaned_group,
            normalized_announcement.as_deref(),
            if normalized_announcement.is_some() {
                Some(safe_now)
            } else {
                None
            },
            safe_now,
        )?;
        updated
            .as_ref()
            .map(|item| self.build_group_detail(item))
            .transpose()
    }

    pub fn list_conversations(
        &self,
        user_id: &str,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<UserWorldConversationView>, i64)> {
        let cleaned_user = user_id.trim();
        if cleaned_user.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let (items, total) = self.storage.list_user_world_conversations(
            cleaned_user,
            offset.max(0),
            normalize_limit(limit, DEFAULT_LIST_LIMIT, 500),
        )?;
        let output = items
            .iter()
            .map(Self::map_conversation_summary)
            .collect::<Vec<_>>();
        Ok((output, total))
    }

    pub fn get_conversation(
        &self,
        user_id: &str,
        conversation_id: &str,
    ) -> Result<Option<UserWorldConversationView>> {
        let cleaned_user = user_id.trim();
        let cleaned_conversation = conversation_id.trim();
        if cleaned_user.is_empty() || cleaned_conversation.is_empty() {
            return Ok(None);
        }
        let conversation = self
            .storage
            .get_user_world_conversation(cleaned_conversation)?;
        let member = self
            .storage
            .get_user_world_member(cleaned_conversation, cleaned_user)?;
        match (conversation, member) {
            (Some(conversation), Some(member)) => {
                Ok(Some(Self::map_conversation_view(&conversation, &member)))
            }
            _ => Ok(None),
        }
    }

    pub fn list_messages(
        &self,
        user_id: &str,
        conversation_id: &str,
        before_message_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<UserWorldMessageView>> {
        let cleaned_user = user_id.trim();
        let cleaned_conversation = conversation_id.trim();
        if cleaned_user.is_empty() || cleaned_conversation.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_member(cleaned_user, cleaned_conversation)?;
        let items = self.storage.list_user_world_messages(
            cleaned_conversation,
            before_message_id,
            normalize_limit(limit, DEFAULT_LIST_LIMIT, 200),
        )?;
        Ok(items.iter().map(Self::map_message).collect())
    }

    pub async fn send_message(
        &self,
        user_id: &str,
        conversation_id: &str,
        content: &str,
        content_type: &str,
        client_msg_id: Option<&str>,
        now: f64,
    ) -> Result<UserWorldSendView> {
        let cleaned_user = user_id.trim();
        let cleaned_conversation = conversation_id.trim();
        if cleaned_user.is_empty() || cleaned_conversation.is_empty() {
            return Err(anyhow!("user_id or conversation_id is empty"));
        }
        let conversation = self
            .storage
            .get_user_world_conversation(cleaned_conversation)?
            .ok_or_else(|| anyhow!("conversation not found"))?;
        self.ensure_member(cleaned_user, cleaned_conversation)?;
        let send_result = self.storage.send_user_world_message(
            cleaned_conversation,
            cleaned_user,
            content,
            content_type,
            client_msg_id,
            now,
        )?;
        let event = send_result.event.as_ref().map(Self::map_realtime_event);
        if let Some(event) = event.clone() {
            let recipients = self
                .storage
                .list_user_world_member_user_ids(&conversation.conversation_id)
                .unwrap_or_default();
            self.publish_to_users(&recipients, event).await;
        }
        Ok(UserWorldSendView {
            message: Self::map_message(&send_result.message),
            inserted: send_result.inserted,
            event,
        })
    }

    pub async fn mark_read(
        &self,
        user_id: &str,
        conversation_id: &str,
        last_read_message_id: Option<i64>,
        now: f64,
    ) -> Result<Option<UserWorldReadView>> {
        let cleaned_user = user_id.trim();
        let cleaned_conversation = conversation_id.trim();
        if cleaned_user.is_empty() || cleaned_conversation.is_empty() {
            return Ok(None);
        }
        let conversation = self
            .storage
            .get_user_world_conversation(cleaned_conversation)?
            .ok_or_else(|| anyhow!("conversation not found"))?;
        self.ensure_member(cleaned_user, cleaned_conversation)?;
        let read_result = self.storage.mark_user_world_read(
            cleaned_conversation,
            cleaned_user,
            last_read_message_id,
            now,
        )?;
        let Some(read_result) = read_result else {
            return Ok(None);
        };
        let event = read_result.event.as_ref().map(Self::map_realtime_event);
        if let Some(event) = event.clone() {
            let recipients = self
                .storage
                .list_user_world_member_user_ids(&conversation.conversation_id)
                .unwrap_or_default();
            self.publish_to_users(&recipients, event).await;
        }
        Ok(Some(Self::map_read_view(read_result, event)))
    }

    pub fn list_events(
        &self,
        user_id: &str,
        conversation_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<UserWorldRealtimeEvent>> {
        let cleaned_user = user_id.trim();
        let cleaned_conversation = conversation_id.trim();
        if cleaned_user.is_empty() || cleaned_conversation.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_member(cleaned_user, cleaned_conversation)?;
        let items = self.storage.list_user_world_events(
            cleaned_conversation,
            after_event_id,
            normalize_limit(limit, DEFAULT_LIST_LIMIT, 500),
        )?;
        Ok(items.iter().map(Self::map_realtime_event).collect())
    }

    pub async fn subscribe_user(
        &self,
        user_id: &str,
    ) -> Result<broadcast::Receiver<UserWorldRealtimeEvent>> {
        let cleaned = user_id.trim();
        if cleaned.is_empty() {
            return Err(anyhow!("user_id is empty"));
        }
        let sender = self.ensure_channel(cleaned).await;
        Ok(sender.subscribe())
    }

    fn ensure_member(&self, user_id: &str, conversation_id: &str) -> Result<UserWorldMemberRecord> {
        self.storage
            .get_user_world_member(conversation_id, user_id)?
            .ok_or_else(|| anyhow!("forbidden"))
    }

    async fn ensure_channel(&self, user_id: &str) -> broadcast::Sender<UserWorldRealtimeEvent> {
        if let Some(sender) = self.channels.read().await.get(user_id).cloned() {
            return sender;
        }
        let mut guard = self.channels.write().await;
        guard
            .entry(user_id.to_string())
            .or_insert_with(|| {
                let (sender, _receiver) = broadcast::channel(256);
                sender
            })
            .clone()
    }

    async fn publish_to_users(&self, user_ids: &[String], event: UserWorldRealtimeEvent) {
        let mut unique = HashSet::new();
        for user_id in user_ids {
            let cleaned = user_id.trim();
            if cleaned.is_empty() || !unique.insert(cleaned.to_string()) {
                continue;
            }
            let sender = self.ensure_channel(cleaned).await;
            let _ = sender.send(event.clone());
        }
    }

    fn map_contact(
        &self,
        user: &UserAccountRecord,
        summary: Option<&UserWorldConversationSummaryRecord>,
    ) -> UserWorldContact {
        UserWorldContact {
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            status: user.status.clone(),
            online: false,
            last_seen_at: None,
            unit_id: user.unit_id.clone(),
            conversation_id: summary.map(|item| item.conversation_id.clone()),
            last_message_preview: summary.and_then(|item| item.last_message_preview.clone()),
            last_message_at: summary.map(|item| item.last_message_at),
            unread_count: summary
                .map(|item| item.unread_count_cache)
                .unwrap_or_default(),
        }
    }

    fn map_conversation_summary(
        summary: &UserWorldConversationSummaryRecord,
    ) -> UserWorldConversationView {
        UserWorldConversationView {
            conversation_id: summary.conversation_id.clone(),
            conversation_type: summary.conversation_type.clone(),
            peer_user_id: summary.peer_user_id.clone(),
            group_id: summary.group_id.clone(),
            group_name: summary.group_name.clone(),
            member_count: summary.member_count,
            last_read_message_id: summary.last_read_message_id,
            unread_count_cache: summary.unread_count_cache,
            pinned: summary.pinned,
            muted: summary.muted,
            updated_at: summary.updated_at,
            last_message_at: summary.last_message_at,
            last_message_id: summary.last_message_id,
            last_message_preview: summary.last_message_preview.clone(),
        }
    }

    fn map_conversation_view(
        conversation: &UserWorldConversationRecord,
        member: &UserWorldMemberRecord,
    ) -> UserWorldConversationView {
        UserWorldConversationView {
            conversation_id: conversation.conversation_id.clone(),
            conversation_type: conversation.conversation_type.clone(),
            peer_user_id: member.peer_user_id.clone(),
            group_id: conversation.group_id.clone(),
            group_name: conversation.group_name.clone(),
            member_count: conversation.member_count,
            last_read_message_id: member.last_read_message_id,
            unread_count_cache: member.unread_count_cache,
            pinned: member.pinned,
            muted: member.muted,
            updated_at: member.updated_at,
            last_message_at: conversation.last_message_at,
            last_message_id: conversation.last_message_id,
            last_message_preview: conversation.last_message_preview.clone(),
        }
    }

    fn build_group_detail(&self, item: &UserWorldGroupRecord) -> Result<UserWorldGroupDetailView> {
        let member_user_ids = self
            .storage
            .list_user_world_member_user_ids(&item.conversation_id)?;
        let mut members = member_user_ids
            .into_iter()
            .map(|user_id| {
                let account = self.storage.get_user_account(&user_id)?;
                Ok(Self::map_group_member(
                    &user_id,
                    account.as_ref(),
                    &item.owner_user_id,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        members.sort_by(|left, right| {
            right
                .is_owner
                .cmp(&left.is_owner)
                .then_with(|| left.username.cmp(&right.username))
                .then_with(|| left.user_id.cmp(&right.user_id))
        });
        Ok(UserWorldGroupDetailView {
            group_id: item.group_id.clone(),
            conversation_id: item.conversation_id.clone(),
            group_name: item.group_name.clone(),
            owner_user_id: item.owner_user_id.clone(),
            announcement: item.announcement.clone(),
            announcement_updated_at: item.announcement_updated_at,
            member_count: members.len().max(item.member_count.max(0) as usize) as i64,
            updated_at: item.updated_at,
            last_message_at: item.last_message_at,
            last_message_id: item.last_message_id,
            last_message_preview: item.last_message_preview.clone(),
            members,
        })
    }

    fn map_group_member(
        user_id: &str,
        account: Option<&UserAccountRecord>,
        owner_user_id: &str,
    ) -> UserWorldGroupMemberView {
        let normalized_user_id = user_id.trim().to_string();
        UserWorldGroupMemberView {
            user_id: normalized_user_id.clone(),
            username: account
                .map(|item| item.username.clone())
                .unwrap_or_else(|| normalized_user_id.clone()),
            status: account
                .map(|item| item.status.clone())
                .unwrap_or_else(|| "active".to_string()),
            unit_id: account.and_then(|item| item.unit_id.clone()),
            is_owner: normalized_user_id == owner_user_id,
        }
    }

    fn map_group(item: &UserWorldGroupRecord) -> UserWorldGroupView {
        UserWorldGroupView {
            group_id: item.group_id.clone(),
            conversation_id: item.conversation_id.clone(),
            group_name: item.group_name.clone(),
            owner_user_id: item.owner_user_id.clone(),
            announcement: item.announcement.clone(),
            announcement_updated_at: item.announcement_updated_at,
            member_count: item.member_count,
            unread_count_cache: item.unread_count_cache,
            updated_at: item.updated_at,
            last_message_at: item.last_message_at,
            last_message_id: item.last_message_id,
            last_message_preview: item.last_message_preview.clone(),
        }
    }

    fn map_message(message: &UserWorldMessageRecord) -> UserWorldMessageView {
        UserWorldMessageView {
            message_id: message.message_id,
            conversation_id: message.conversation_id.clone(),
            sender_user_id: message.sender_user_id.clone(),
            content: message.content.clone(),
            content_type: message.content_type.clone(),
            client_msg_id: message.client_msg_id.clone(),
            created_at: message.created_at,
        }
    }

    fn map_realtime_event(event: &UserWorldEventRecord) -> UserWorldRealtimeEvent {
        UserWorldRealtimeEvent {
            conversation_id: event.conversation_id.clone(),
            event_id: event.event_id,
            event_type: event.event_type.clone(),
            payload: event.payload.clone(),
            created_time: event.created_time,
        }
    }

    fn map_read_view(
        read_result: UserWorldReadResult,
        event: Option<UserWorldRealtimeEvent>,
    ) -> UserWorldReadView {
        UserWorldReadView {
            conversation_id: read_result.member.conversation_id,
            user_id: read_result.member.user_id,
            peer_user_id: read_result.member.peer_user_id,
            last_read_message_id: read_result.member.last_read_message_id,
            unread_count_cache: read_result.member.unread_count_cache,
            updated_at: read_result.member.updated_at,
            event,
        }
    }
}

fn normalize_limit(limit: i64, default_limit: i64, max_limit: i64) -> i64 {
    if limit <= 0 {
        default_limit
    } else {
        limit.min(max_limit).max(1)
    }
}
