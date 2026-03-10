use crate::storage::BeeroomChatMessageRecord;
use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize)]
pub struct BeeroomRealtimeEvent {
    pub event_id: i64,
    pub user_id: String,
    pub group_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: f64,
}

pub struct BeeroomRealtimeService {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<BeeroomRealtimeEvent>>>>,
    sequence: AtomicI64,
}

impl Default for BeeroomRealtimeService {
    fn default() -> Self {
        Self::new()
    }
}

impl BeeroomRealtimeService {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            sequence: AtomicI64::new(0),
        }
    }

    pub async fn subscribe_group(
        &self,
        user_id: &str,
        group_id: &str,
    ) -> Result<broadcast::Receiver<BeeroomRealtimeEvent>> {
        let normalized_key = normalize_channel_key(user_id, group_id)?;
        let sender = self.ensure_channel(&normalized_key).await;
        Ok(sender.subscribe())
    }

    pub async fn publish_chat_message(&self, record: &BeeroomChatMessageRecord) {
        let event = BeeroomRealtimeEvent {
            event_id: self.next_event_id(),
            user_id: record.user_id.clone(),
            group_id: record.group_id.clone(),
            event_type: "chat_message".to_string(),
            payload: chat_message_payload(record),
            created_at: record.created_at,
        };
        let target_user_id = event.user_id.clone();
        let target_group_id = event.group_id.clone();
        self.publish(&target_user_id, &target_group_id, event).await;
    }

    pub async fn publish_chat_cleared(
        &self,
        user_id: &str,
        group_id: &str,
        deleted: i64,
        created_at: f64,
    ) {
        let normalized_user = user_id.trim();
        let normalized_group = group_id.trim();
        if normalized_user.is_empty() || normalized_group.is_empty() {
            return;
        }
        let event = BeeroomRealtimeEvent {
            event_id: self.next_event_id(),
            user_id: normalized_user.to_string(),
            group_id: normalized_group.to_string(),
            event_type: "chat_cleared".to_string(),
            payload: json!({
                "group_id": normalized_group,
                "deleted": deleted.max(0),
            }),
            created_at,
        };
        self.publish(normalized_user, normalized_group, event).await;
    }

    fn next_event_id(&self) -> i64 {
        self.sequence.fetch_add(1, Ordering::Relaxed) + 1
    }

    async fn publish(&self, user_id: &str, group_id: &str, event: BeeroomRealtimeEvent) {
        let Ok(channel_key) = normalize_channel_key(user_id, group_id) else {
            return;
        };
        let sender = self.ensure_channel(&channel_key).await;
        let _ = sender.send(event);
    }

    async fn ensure_channel(&self, channel_key: &str) -> broadcast::Sender<BeeroomRealtimeEvent> {
        if let Some(sender) = self.channels.read().await.get(channel_key).cloned() {
            return sender;
        }
        let mut guard = self.channels.write().await;
        guard
            .entry(channel_key.to_string())
            .or_insert_with(|| {
                let (sender, _receiver) = broadcast::channel(512);
                sender
            })
            .clone()
    }
}

fn normalize_channel_key(user_id: &str, group_id: &str) -> Result<String> {
    let normalized_user = user_id.trim();
    let normalized_group = group_id.trim();
    if normalized_user.is_empty() || normalized_group.is_empty() {
        return Err(anyhow!("user_id/group_id is required"));
    }
    Ok(format!("{normalized_user}::{normalized_group}"))
}

fn chat_message_payload(record: &BeeroomChatMessageRecord) -> serde_json::Value {
    json!({
        "message_id": record.message_id,
        "user_id": record.user_id,
        "group_id": record.group_id,
        "sender_kind": record.sender_kind,
        "sender_name": record.sender_name,
        "sender_agent_id": record.sender_agent_id,
        "mention_name": record.mention_name,
        "mention_agent_id": record.mention_agent_id,
        "body": record.body,
        "meta": record.meta,
        "tone": record.tone,
        "client_msg_id": record.client_msg_id,
        "created_at": record.created_at,
    })
}
