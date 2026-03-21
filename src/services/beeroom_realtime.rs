use crate::storage::BeeroomChatMessageRecord;
use anyhow::{anyhow, Result};
use chrono::Utc;
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

#[derive(Clone)]
struct BeeroomRealtimeChannel {
    sender: broadcast::Sender<BeeroomRealtimeEvent>,
    last_event_id: i64,
}

pub struct BeeroomRealtimeService {
    channels: Arc<RwLock<HashMap<String, BeeroomRealtimeChannel>>>,
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

    pub async fn latest_event_id(&self, user_id: &str, group_id: &str) -> Result<i64> {
        let normalized_key = normalize_channel_key(user_id, group_id)?;
        Ok(self
            .channels
            .read()
            .await
            .get(&normalized_key)
            .map_or(0, |channel| channel.last_event_id))
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

    pub async fn publish_group_event(
        &self,
        user_id: &str,
        group_id: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) {
        let normalized_user = user_id.trim();
        let normalized_group = group_id.trim();
        let normalized_event_type = event_type.trim();
        if normalized_user.is_empty()
            || normalized_group.is_empty()
            || normalized_event_type.is_empty()
        {
            return;
        }
        let event = BeeroomRealtimeEvent {
            event_id: self.next_event_id(),
            user_id: normalized_user.to_string(),
            group_id: normalized_group.to_string(),
            event_type: normalized_event_type.to_string(),
            payload,
            created_at: now_ts(),
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
        let sender = {
            let mut guard = self.channels.write().await;
            let channel = guard.entry(channel_key).or_insert_with(|| {
                let (sender, _receiver) = broadcast::channel(512);
                BeeroomRealtimeChannel {
                    sender,
                    last_event_id: 0,
                }
            });
            channel.last_event_id = channel.last_event_id.max(event.event_id);
            channel.sender.clone()
        };
        let _ = sender.send(event);
    }

    async fn ensure_channel(&self, channel_key: &str) -> broadcast::Sender<BeeroomRealtimeEvent> {
        if let Some(channel) = self.channels.read().await.get(channel_key).cloned() {
            return channel.sender;
        }
        let mut guard = self.channels.write().await;
        guard
            .entry(channel_key.to_string())
            .or_insert_with(|| {
                let (sender, _receiver) = broadcast::channel(512);
                BeeroomRealtimeChannel {
                    sender,
                    last_event_id: 0,
                }
            })
            .sender
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

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::BeeroomRealtimeService;
    use crate::storage::BeeroomChatMessageRecord;
    use serde_json::json;
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};

    fn sample_chat_message_record() -> BeeroomChatMessageRecord {
        BeeroomChatMessageRecord {
            message_id: 88,
            user_id: "u_a".to_string(),
            group_id: "g_a".to_string(),
            sender_kind: "agent".to_string(),
            sender_name: "Planner".to_string(),
            sender_agent_id: Some("agent_planner".to_string()),
            mention_name: None,
            mention_agent_id: None,
            body: "task ready".to_string(),
            meta: Some("{\"cost\":1}".to_string()),
            tone: "neutral".to_string(),
            client_msg_id: Some("client-88".to_string()),
            created_at: 1234.5,
        }
    }

    #[tokio::test]
    async fn subscribe_group_rejects_empty_identifiers() {
        let service = BeeroomRealtimeService::new();
        assert!(service.subscribe_group("", "group-a").await.is_err());
        assert!(service.subscribe_group("user-a", "").await.is_err());
    }

    #[tokio::test]
    async fn subscribe_group_isolated_by_user_and_group() {
        let service = BeeroomRealtimeService::new();
        let mut target = service
            .subscribe_group("user-a", "group-a")
            .await
            .expect("target subscription should succeed");
        let mut other_user = service
            .subscribe_group("user-b", "group-a")
            .await
            .expect("other user subscription should succeed");
        let mut other_group = service
            .subscribe_group("user-a", "group-b")
            .await
            .expect("other group subscription should succeed");

        service
            .publish_group_event(
                "user-a",
                "group-a",
                "team_task_dispatch",
                json!({ "task_id": "task-1" }),
            )
            .await;

        let event = target
            .recv()
            .await
            .expect("target receiver should receive event");
        assert_eq!(event.user_id, "user-a");
        assert_eq!(event.group_id, "group-a");
        assert_eq!(event.event_type, "team_task_dispatch");

        assert!(matches!(other_user.try_recv(), Err(TryRecvError::Empty)));
        assert!(matches!(other_group.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn publish_events_use_monotonic_event_id_sequence() {
        let service = BeeroomRealtimeService::new();
        let mut receiver = service
            .subscribe_group("u_a", "g_a")
            .await
            .expect("subscription should succeed");

        service.publish_chat_cleared("u_a", "g_a", 3, 10.0).await;
        service
            .publish_group_event("u_a", "g_a", "team_task_dispatch", json!({ "n": 1 }))
            .await;
        let record = sample_chat_message_record();
        service.publish_chat_message(&record).await;

        let first = receiver.recv().await.expect("first event should exist");
        let second = receiver.recv().await.expect("second event should exist");
        let third = receiver.recv().await.expect("third event should exist");

        assert_eq!(first.event_type, "chat_cleared");
        assert_eq!(second.event_type, "team_task_dispatch");
        assert_eq!(third.event_type, "chat_message");
        assert!(first.event_id < second.event_id);
        assert!(second.event_id < third.event_id);
    }

    #[tokio::test]
    async fn latest_event_id_tracks_per_channel_state() {
        let service = BeeroomRealtimeService::new();
        assert_eq!(
            service
                .latest_event_id("user-a", "group-a")
                .await
                .expect("lookup should succeed"),
            0
        );

        service
            .publish_group_event(
                "user-a",
                "group-a",
                "team_task_dispatch",
                json!({ "task_id": "a-1" }),
            )
            .await;
        service
            .publish_group_event(
                "user-a",
                "group-b",
                "team_task_dispatch",
                json!({ "task_id": "b-1" }),
            )
            .await;

        let group_a_latest = service
            .latest_event_id("user-a", "group-a")
            .await
            .expect("group-a lookup should succeed");
        let group_b_latest = service
            .latest_event_id("user-a", "group-b")
            .await
            .expect("group-b lookup should succeed");
        assert!(group_a_latest > 0);
        assert!(group_b_latest > 0);
        assert_ne!(group_a_latest, group_b_latest);
    }

    #[tokio::test]
    async fn latest_event_id_rejects_empty_identifiers() {
        let service = BeeroomRealtimeService::new();
        assert!(service.latest_event_id("", "group-a").await.is_err());
        assert!(service.latest_event_id("user-a", "").await.is_err());
    }

    #[tokio::test]
    async fn lagged_receiver_reports_gap_after_burst_publish() {
        let service = BeeroomRealtimeService::new();
        let mut receiver = service
            .subscribe_group("user-a", "group-a")
            .await
            .expect("subscription should succeed");

        for seq in 0..700 {
            service
                .publish_group_event(
                    "user-a",
                    "group-a",
                    "team_task_dispatch",
                    json!({ "seq": seq }),
                )
                .await;
        }

        match receiver.recv().await {
            Err(RecvError::Lagged(skipped)) => {
                assert!(skipped > 0);
                let recovered = receiver
                    .recv()
                    .await
                    .expect("receiver should continue after lagged notification");
                assert_eq!(recovered.event_type, "team_task_dispatch");
            }
            other => panic!("expected lagged error, got {other:?}"),
        }
    }
}
