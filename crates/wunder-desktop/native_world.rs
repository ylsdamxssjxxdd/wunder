use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use wunder_server::UserWorldRealtimeEvent;

const WORLD_EVENT_CHANNEL_CAPACITY: usize = 128;
const WORLD_MESSAGE_PAGE_LIMIT: i64 = 100;
const WORLD_EVENT_REPLAY_LIMIT: i64 = 200;
const WORLD_GROUP_MEMBER_LIMIT: usize = 100;
const WORLD_GROUP_ANNOUNCEMENT_LIMIT: usize = 4_000;

#[derive(Clone, Debug)]
pub struct WorldContact {
    pub user_id: String,
    pub username: String,
    pub online: bool,
    pub conversation_id: String,
    pub preview: String,
    pub unread: i64,
}

#[derive(Clone, Debug)]
pub struct WorldGroup {
    pub group_id: String,
    pub conversation_id: String,
    pub name: String,
    pub members: i64,
    pub preview: String,
    pub unread: i64,
}

#[derive(Clone, Debug)]
pub struct WorldMessage {
    pub id: i64,
    pub sender: String,
    pub content: String,
    pub created_at: f64,
    pub mine: bool,
}

#[derive(Clone, Debug)]
pub struct WorldEvent {
    pub conversation_id: String,
    pub event_id: i64,
    pub event_type: String,
    pub message: Option<WorldMessage>,
}

#[derive(Clone, Debug)]
pub struct WorldGroupMember {
    pub user_id: String,
    pub username: String,
    pub is_owner: bool,
}

#[derive(Clone, Debug)]
pub struct WorldGroupDetail {
    pub group_id: String,
    pub conversation_id: String,
    pub name: String,
    pub owner_user_id: String,
    pub announcement: String,
    pub members: Vec<WorldGroupMember>,
}

/// Bounded realtime feed bridging the runtime broadcast channel to the UI
/// thread. The queue never grows: when the UI falls behind, the feed flags an
/// overflow and the UI recovers with a snapshot reload instead of trusting
/// incremental updates.
pub struct WorldEventFeed {
    receiver: mpsc::Receiver<WorldEvent>,
    overflowed: Arc<AtomicBool>,
}

impl WorldEventFeed {
    /// Drain up to `max` pending events without blocking. Called from the UI
    /// thread inside a timer tick so one tick can never stall on a large batch.
    pub fn drain(&self, max: usize) -> Vec<WorldEvent> {
        let mut events = Vec::new();
        while events.len() < max {
            match self.receiver.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }
        events
    }

    /// Returns and clears the overflow flag. The UI must reload snapshots for
    /// the affected conversations after an overflow.
    pub fn take_overflowed(&self) -> bool {
        self.overflowed.swap(false, Ordering::AcqRel)
    }
}

/// Tracks the newest message id already projected into the UI per
/// conversation, so a live event echo of an already-applied message is
/// dropped instead of rendered twice.
#[derive(Default)]
pub struct WorldMessageTracker {
    last_message_id: Mutex<HashMap<String, i64>>,
}

impl WorldMessageTracker {
    pub fn observe(&self, conversation_id: &str, message_id: i64) {
        let mut guard = self
            .last_message_id
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(conversation_id.trim().to_string()).or_insert(0);
        if message_id > *entry {
            *entry = message_id;
        }
    }

    pub fn seen(&self, conversation_id: &str, message_id: i64) -> bool {
        let guard = self
            .last_message_id
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        message_id <= guard.get(conversation_id.trim()).copied().unwrap_or(0)
    }
}

impl NativeDesktop {
    pub fn list_world_contacts(
        &self,
        keyword: &str,
        offset: i64,
    ) -> Result<(Vec<WorldContact>, i64)> {
        let (items, total) = self.state().projection.user_world.list_contacts(
            self.user_id(),
            (!keyword.trim().is_empty()).then_some(keyword.trim()),
            offset.max(0),
            100,
        )?;
        Ok((
            items
                .into_iter()
                .map(|item| WorldContact {
                    user_id: item.user_id,
                    username: item.username,
                    online: item.online,
                    conversation_id: item.conversation_id.unwrap_or_default(),
                    preview: item.last_message_preview.unwrap_or_default(),
                    unread: item.unread_count,
                })
                .collect(),
            total,
        ))
    }

    pub fn list_world_groups(&self, offset: i64) -> Result<(Vec<WorldGroup>, i64)> {
        let (items, total) =
            self.state()
                .projection
                .user_world
                .list_groups(self.user_id(), offset.max(0), 100)?;
        Ok((
            items
                .into_iter()
                .map(|item| WorldGroup {
                    group_id: item.group_id,
                    conversation_id: item.conversation_id,
                    name: item.group_name,
                    members: item.member_count,
                    preview: item.last_message_preview.unwrap_or_default(),
                    unread: item.unread_count_cache,
                })
                .collect(),
            total,
        ))
    }

    pub fn create_world_direct_conversation(&self, peer_user_id: &str) -> Result<String> {
        let item = self
            .state()
            .projection
            .user_world
            .resolve_or_create_direct_conversation(self.user_id(), peer_user_id, now())?;
        Ok(item.conversation_id)
    }

    pub fn create_world_group(&self, name: &str, member_user_ids: &[String]) -> Result<String> {
        let item = self.state().projection.user_world.create_group(
            self.user_id(),
            name,
            member_user_ids,
            now(),
        )?;
        Ok(item.conversation_id)
    }

    /// Load one page of conversation messages in chronological order (oldest
    /// first). `before_message_id` pages backwards through history; storage
    /// returns newest-first, so the result is reversed for direct UI use.
    pub fn list_world_messages(
        &self,
        conversation_id: &str,
        before_message_id: Option<i64>,
    ) -> Result<Vec<WorldMessage>> {
        let items = self.state().projection.user_world.list_messages(
            self.user_id(),
            conversation_id,
            before_message_id,
            WORLD_MESSAGE_PAGE_LIMIT,
        )?;
        let mut messages: Vec<WorldMessage> = items
            .into_iter()
            .map(|item| WorldMessage {
                id: item.message_id,
                sender: item.sender_user_id.clone(),
                content: item.content,
                created_at: item.created_at,
                mine: item.sender_user_id == self.user_id(),
            })
            .collect();
        messages.reverse();
        Ok(messages)
    }

    /// Whether another older page exists for the conversation (the page came
    /// back full). Keeps the UI from offering a dead "load earlier" action.
    pub fn has_older_world_messages(
        &self,
        conversation_id: &str,
        oldest_message_id: i64,
    ) -> Result<bool> {
        if oldest_message_id <= 1 {
            return Ok(false);
        }
        let items = self.state().projection.user_world.list_messages(
            self.user_id(),
            conversation_id,
            Some(oldest_message_id),
            1,
        )?;
        Ok(!items.is_empty())
    }

    pub fn mark_world_read(
        &self,
        conversation_id: &str,
        last_message_id: Option<i64>,
    ) -> Result<()> {
        self.runtime
            .block_on(self.state().projection.user_world.mark_read(
                self.user_id(),
                conversation_id,
                last_message_id,
                now(),
            ))?;
        Ok(())
    }

    pub fn send_world_message(&self, conversation_id: &str, content: &str) -> Result<WorldMessage> {
        let result = self
            .runtime
            .block_on(self.state().projection.user_world.send_message(
                self.user_id(),
                conversation_id,
                content,
                "text",
                None,
                now(),
            ))?;
        let item = result.message;
        Ok(WorldMessage {
            id: item.message_id,
            sender: item.sender_user_id.clone(),
            content: item.content,
            created_at: item.created_at,
            mine: item.sender_user_id == self.user_id(),
        })
    }

    /// Total unread messages across every conversation of the signed-in user.
    /// Backs the rail badge; the underlying scan is bounded in the service.
    pub fn total_world_unread(&self) -> Result<i64> {
        self.state().projection.user_world.total_unread(self.user_id())
    }

    pub fn get_world_group_detail(&self, group_id: &str) -> Result<WorldGroupDetail> {
        let detail = self
            .state()
            .projection
            .user_world
            .get_group_detail(self.user_id(), group_id.trim())?
            .ok_or_else(|| anyhow!("群组不存在或无权访问"))?;
        Ok(WorldGroupDetail {
            group_id: detail.group_id,
            conversation_id: detail.conversation_id,
            name: detail.group_name,
            owner_user_id: detail.owner_user_id,
            announcement: detail.announcement.unwrap_or_default(),
            members: detail
                .members
                .into_iter()
                .take(WORLD_GROUP_MEMBER_LIMIT)
                .map(|member| WorldGroupMember {
                    user_id: member.user_id,
                    username: member.username,
                    is_owner: member.is_owner,
                })
                .collect(),
        })
    }

    pub fn update_world_group_announcement(
        &self,
        group_id: &str,
        announcement: &str,
    ) -> Result<()> {
        let trimmed = announcement.trim();
        if trimmed.chars().count() > WORLD_GROUP_ANNOUNCEMENT_LIMIT {
            return Err(anyhow!(
                "公告过长（最多 {WORLD_GROUP_ANNOUNCEMENT_LIMIT} 字）"
            ));
        }
        let text = (!trimmed.is_empty()).then_some(trimmed);
        let updated = self
            .state()
            .projection
            .user_world
            .update_group_announcement(self.user_id(), group_id.trim(), text, now())?;
        if updated.is_none() {
            return Err(anyhow!("群组不存在或无权访问"));
        }
        Ok(())
    }

    /// Replay events for one conversation after a cursor. Used to recover
    /// missed updates after a snapshot reload or an event feed overflow.
    pub fn list_world_events(
        &self,
        conversation_id: &str,
        after_event_id: i64,
    ) -> Result<Vec<WorldEvent>> {
        let user = self.user_id().to_string();
        let items = self.state().projection.user_world.list_events(
            self.user_id(),
            conversation_id,
            after_event_id,
            WORLD_EVENT_REPLAY_LIMIT,
        )?;
        Ok(items
            .into_iter()
            .map(|event| world_event_from_service(&user, event))
            .collect())
    }

    /// Start the shared realtime feed for the signed-in user. The subscription
    /// lives for the whole process (the desktop equivalent of the web client's
    /// shared connection); page switches never cancel it.
    pub fn start_world_event_feed(&self) -> Result<WorldEventFeed> {
        let user = self.user_id().to_string();
        let mut receiver = self
            .runtime
            .block_on(self.state().projection.user_world.subscribe_user(&user))?;
        let (sender, receiver_out) = mpsc::sync_channel::<WorldEvent>(WORLD_EVENT_CHANNEL_CAPACITY);
        let overflowed = Arc::new(AtomicBool::new(false));
        let overflow_flag = overflowed.clone();
        std::thread::Builder::new()
            .name("world-event-feed".into())
            .spawn(move || loop {
                match receiver.blocking_recv() {
                    Ok(event) => {
                        let event = world_event_from_service(&user, event);
                        if sender.try_send(event).is_err() {
                            overflow_flag.store(true, Ordering::Release);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        overflow_flag.store(true, Ordering::Release);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            })?;
        Ok(WorldEventFeed {
            receiver: receiver_out,
            overflowed,
        })
    }
}

fn world_event_from_service(user: &str, event: UserWorldRealtimeEvent) -> WorldEvent {
    let message = (event.event_type == "uw.message")
        .then(|| event.payload.get("message"))
        .flatten()
        .and_then(|payload| world_message_from_payload(user, payload));
    WorldEvent {
        conversation_id: event.conversation_id,
        event_id: event.event_id,
        event_type: event.event_type,
        message,
    }
}

fn world_message_from_payload(user: &str, payload: &Value) -> Option<WorldMessage> {
    let sender = payload.get("sender_user_id")?.as_str()?.to_string();
    Some(WorldMessage {
        id: payload.get("message_id")?.as_i64()?,
        sender: sender.clone(),
        content: payload
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        created_at: payload
            .get("created_at")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
        mine: sender == user,
    })
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_dedupes_and_ignores_stale_ids() {
        let tracker = WorldMessageTracker::default();
        assert!(!tracker.seen("c1", 3));
        tracker.observe("c1", 3);
        assert!(tracker.seen("c1", 3));
        assert!(tracker.seen("c1", 2));
        assert!(!tracker.seen("c1", 4));
        // A stale echo must never lower the cursor.
        tracker.observe("c1", 1);
        assert!(tracker.seen("c1", 3));
        assert!(!tracker.seen("c2", 3));
    }

    #[test]
    fn world_message_payload_projection_marks_mine() {
        let payload = serde_json::json!({
            "message_id": 7,
            "sender_user_id": "u1",
            "content": "hello",
            "created_at": 12.0
        });
        let mine = world_message_from_payload("u1", &payload).expect("payload projects");
        assert!(mine.mine);
        assert_eq!(mine.id, 7);
        assert_eq!(mine.content, "hello");
        let other = world_message_from_payload("u2", &payload).expect("payload projects");
        assert!(!other.mine);
        assert!(world_message_from_payload("u1", &serde_json::json!({})).is_none());
    }
}
