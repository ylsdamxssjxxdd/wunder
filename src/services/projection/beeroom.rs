use crate::services::directory::{RouteLeaseGuard, RouteLeaseService, RouteTargetKind};
use crate::services::stream_events::StreamEventService;
use crate::storage::{BeeroomChatMessageRecord, StorageBackend};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{broadcast, RwLock};
use tracing::warn;
use uuid::Uuid;

const PROJECTION_CHANNEL_CAPACITY: usize = 512;
const REPLAY_LIMIT_DEFAULT: i64 = 200;
const REPLAY_LIMIT_MAX: i64 = 1000;

#[derive(Debug, Clone, Serialize)]
pub struct BeeroomProjectionMetricsSnapshot {
    pub publish_total: u64,
    pub replay_batch_total: u64,
    pub replay_event_total: u64,
    pub replay_failure_total: u64,
    pub lag_recovery_total: u64,
    pub push_sample_total: u64,
    pub push_latency_avg_ms: f64,
    pub push_latency_max_ms: u64,
}

#[derive(Default)]
struct BeeroomProjectionMetrics {
    publish_total: AtomicU64,
    replay_batch_total: AtomicU64,
    replay_event_total: AtomicU64,
    replay_failure_total: AtomicU64,
    lag_recovery_total: AtomicU64,
    push_sample_total: AtomicU64,
    push_latency_total_ms: AtomicU64,
    push_latency_max_ms: AtomicU64,
}

impl BeeroomProjectionMetrics {
    fn record_publish(&self) {
        self.publish_total.fetch_add(1, Ordering::Relaxed);
    }

    fn record_replay_batch(&self, event_count: usize) {
        self.replay_batch_total.fetch_add(1, Ordering::Relaxed);
        if event_count > 0 {
            self.replay_event_total
                .fetch_add(event_count as u64, Ordering::Relaxed);
        }
    }

    fn record_replay_failure(&self) {
        self.replay_failure_total.fetch_add(1, Ordering::Relaxed);
    }

    fn record_lag_recovery(&self) {
        self.lag_recovery_total.fetch_add(1, Ordering::Relaxed);
    }

    fn record_push_latency_ms(&self, latency_ms: u64) {
        self.push_sample_total.fetch_add(1, Ordering::Relaxed);
        self.push_latency_total_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        let mut current = self.push_latency_max_ms.load(Ordering::Relaxed);
        while latency_ms > current {
            match self.push_latency_max_ms.compare_exchange_weak(
                current,
                latency_ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }

    fn snapshot(&self) -> BeeroomProjectionMetricsSnapshot {
        let push_sample_total = self.push_sample_total.load(Ordering::Relaxed);
        let push_latency_total_ms = self.push_latency_total_ms.load(Ordering::Relaxed);
        let push_latency_avg_ms = if push_sample_total == 0 {
            0.0
        } else {
            push_latency_total_ms as f64 / push_sample_total as f64
        };
        BeeroomProjectionMetricsSnapshot {
            publish_total: self.publish_total.load(Ordering::Relaxed),
            replay_batch_total: self.replay_batch_total.load(Ordering::Relaxed),
            replay_event_total: self.replay_event_total.load(Ordering::Relaxed),
            replay_failure_total: self.replay_failure_total.load(Ordering::Relaxed),
            lag_recovery_total: self.lag_recovery_total.load(Ordering::Relaxed),
            push_sample_total,
            push_latency_avg_ms,
            push_latency_max_ms: self.push_latency_max_ms.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BeeroomProjectionEvent {
    pub event_id: i64,
    pub user_id: String,
    pub group_id: String,
    pub event_type: String,
    pub payload: Value,
    pub created_at: f64,
}

#[derive(Clone)]
struct BeeroomProjectionChannel {
    sender: broadcast::Sender<BeeroomProjectionEvent>,
    last_event_id: i64,
}

pub struct BeeroomProjectionService {
    stream_events: Arc<StreamEventService>,
    route_leases: Arc<RouteLeaseService>,
    channels: Arc<RwLock<HashMap<String, BeeroomProjectionChannel>>>,
    route_guards: StdRwLock<HashMap<String, RouteLeaseGuard>>,
    metrics: Arc<BeeroomProjectionMetrics>,
    runtime_owner_id: String,
}

impl BeeroomProjectionService {
    pub fn new(storage: Arc<dyn StorageBackend>, route_leases: Arc<RouteLeaseService>) -> Self {
        Self {
            stream_events: Arc::new(StreamEventService::new(storage)),
            route_leases,
            channels: Arc::new(RwLock::new(HashMap::new())),
            route_guards: StdRwLock::new(HashMap::new()),
            metrics: Arc::new(BeeroomProjectionMetrics::default()),
            runtime_owner_id: format!("beeroom_projection_{}", Uuid::new_v4().simple()),
        }
    }

    pub fn route_target_id(user_id: &str, group_id: &str) -> Result<String> {
        normalize_stream_key(user_id, group_id)
    }

    pub fn metrics_snapshot(&self) -> BeeroomProjectionMetricsSnapshot {
        self.metrics.snapshot()
    }

    pub fn record_replay_batch(&self, event_count: usize) {
        self.metrics.record_replay_batch(event_count);
    }

    pub fn record_replay_failure(&self) {
        self.metrics.record_replay_failure();
    }

    pub fn record_lag_recovery(&self) {
        self.metrics.record_lag_recovery();
    }

    pub fn record_push_latency_sample(&self, created_at: f64) {
        if !created_at.is_finite() || created_at <= 0.0 {
            return;
        }
        let now = now_ts();
        let latency_ms = ((now - created_at).max(0.0) * 1000.0) as u64;
        self.metrics.record_push_latency_ms(latency_ms);
    }

    pub async fn subscribe_group(
        &self,
        user_id: &str,
        group_id: &str,
    ) -> Result<broadcast::Receiver<BeeroomProjectionEvent>> {
        let stream_key = normalize_stream_key(user_id, group_id)?;
        let sender = self.ensure_channel(user_id, group_id, &stream_key).await;
        Ok(sender.subscribe())
    }

    pub async fn latest_event_id(&self, user_id: &str, group_id: &str) -> Result<i64> {
        let stream_key = normalize_stream_key(user_id, group_id)?;
        let cached = self
            .channels
            .read()
            .await
            .get(&stream_key)
            .map_or(0, |channel| channel.last_event_id);
        if cached > 0 {
            return Ok(cached);
        }
        let tail = self.stream_events.tail_event_id(&stream_key).await?;
        Ok(tail)
    }

    pub async fn list_group_events(
        &self,
        user_id: &str,
        group_id: &str,
        after_event_id: i64,
        limit: i64,
    ) -> Result<Vec<BeeroomProjectionEvent>> {
        let normalized_user = user_id.trim();
        let normalized_group = group_id.trim();
        let stream_key = normalize_stream_key(normalized_user, normalized_group)?;
        let safe_limit = normalize_replay_limit(limit);
        // Replay always comes from durable storage so reconnect/lag can recover without full refresh.
        let records = self
            .stream_events
            .list_events(&stream_key, after_event_id.max(0), safe_limit.max(1))
            .await?;
        let events = records
            .into_iter()
            .filter_map(|record| map_record_to_event(record, normalized_user, normalized_group))
            .collect::<Vec<_>>();
        Ok(events)
    }

    pub async fn publish_chat_message(&self, record: &BeeroomChatMessageRecord) {
        if let Err(err) = self
            .publish_event(
                &record.user_id,
                &record.group_id,
                "chat_message",
                chat_message_payload(record),
                record.created_at,
            )
            .await
        {
            warn!(
                "publish beeroom chat_message failed: user_id={}, group_id={}, error={err}",
                record.user_id, record.group_id
            );
        }
    }

    pub async fn publish_chat_cleared(
        &self,
        user_id: &str,
        group_id: &str,
        deleted: i64,
        created_at: f64,
    ) {
        let payload = json!({
            "group_id": group_id.trim(),
            "deleted": deleted.max(0),
        });
        if let Err(err) = self
            .publish_event(user_id, group_id, "chat_cleared", payload, created_at)
            .await
        {
            warn!(
                "publish beeroom chat_cleared failed: user_id={}, group_id={}, error={err}",
                user_id, group_id
            );
        }
    }

    pub async fn publish_group_event(
        &self,
        user_id: &str,
        group_id: &str,
        event_type: &str,
        payload: Value,
    ) {
        if let Err(err) = self
            .publish_event(user_id, group_id, event_type, payload, now_ts())
            .await
        {
            warn!(
                "publish beeroom group event failed: user_id={}, group_id={}, event_type={}, error={err}",
                user_id, group_id, event_type
            );
        }
    }

    async fn publish_event(
        &self,
        user_id: &str,
        group_id: &str,
        event_type: &str,
        payload: Value,
        created_at: f64,
    ) -> Result<()> {
        let normalized_user = user_id.trim();
        let normalized_group = group_id.trim();
        let normalized_event_type = event_type.trim();
        if normalized_user.is_empty()
            || normalized_group.is_empty()
            || normalized_event_type.is_empty()
        {
            return Ok(());
        }
        let stream_key = normalize_stream_key(normalized_user, normalized_group)?;
        let sender = self
            .ensure_channel(normalized_user, normalized_group, &stream_key)
            .await;
        let event_id = self
            .persist_stream_event(
                stream_key.clone(),
                normalized_user.to_string(),
                normalized_event_type.to_string(),
                payload.clone(),
                created_at,
            )
            .await?;
        let event = BeeroomProjectionEvent {
            event_id,
            user_id: normalized_user.to_string(),
            group_id: normalized_group.to_string(),
            event_type: normalized_event_type.to_string(),
            payload,
            created_at,
        };
        self.metrics.record_publish();
        let _ = sender.send(event);
        Ok(())
    }

    async fn persist_stream_event(
        &self,
        stream_key: String,
        user_id: String,
        event_type: String,
        payload: Value,
        created_at: f64,
    ) -> Result<i64> {
        let session_id = stream_key.clone();
        let envelope = json!({
            "event": event_type,
            "data": payload,
            "timestamp": to_rfc3339(created_at),
        });
        let event_id = self
            .stream_events
            .append_event(&session_id, &user_id, envelope)
            .await?;
        self.update_channel_tail_if_present(&stream_key, event_id)
            .await;
        Ok(event_id)
    }

    async fn ensure_channel(
        &self,
        user_id: &str,
        group_id: &str,
        stream_key: &str,
    ) -> broadcast::Sender<BeeroomProjectionEvent> {
        if let Some(channel) = self.channels.read().await.get(stream_key).cloned() {
            return channel.sender;
        }
        let mut guard = self.channels.write().await;
        if let Some(channel) = guard.get(stream_key).cloned() {
            return channel.sender;
        }
        self.ensure_projection_route_guard(user_id, group_id, stream_key);
        let (sender, _receiver) = broadcast::channel(PROJECTION_CHANNEL_CAPACITY);
        guard.insert(
            stream_key.to_string(),
            BeeroomProjectionChannel {
                sender: sender.clone(),
                last_event_id: 0,
            },
        );
        sender
    }

    async fn update_channel_tail_if_present(&self, stream_key: &str, event_id: i64) {
        if event_id <= 0 {
            return;
        }
        let mut guard = self.channels.write().await;
        if let Some(channel) = guard.get_mut(stream_key) {
            channel.last_event_id = channel.last_event_id.max(event_id);
        }
    }

    fn ensure_projection_route_guard(&self, user_id: &str, group_id: &str, stream_key: &str) {
        let Some(mut guard) = self.route_guards.write().ok() else {
            return;
        };
        if guard.contains_key(stream_key) {
            return;
        }
        let Some(route_lease) = self.route_leases.try_acquire_route_lease(
            RouteTargetKind::Projection,
            stream_key,
            &self.runtime_owner_id,
            None,
            Some(user_id),
        ) else {
            let snapshot = self
                .route_leases
                .route_snapshot(RouteTargetKind::Projection, stream_key);
            warn!(
                "beeroom projection route lease acquisition skipped: user_id={}, group_id={}, stream_key={}, owner={:?}",
                user_id,
                group_id,
                stream_key,
                snapshot.as_ref().map(|item| item.owner_id.as_str()),
            );
            return;
        };
        guard.insert(stream_key.to_string(), route_lease);
    }
}

fn normalize_stream_key(user_id: &str, group_id: &str) -> Result<String> {
    let normalized_user = user_id.trim();
    let normalized_group = group_id.trim();
    if normalized_user.is_empty() || normalized_group.is_empty() {
        return Err(anyhow!("user_id/group_id is required"));
    }
    Ok(format!("beeroom::{normalized_user}::{normalized_group}"))
}

fn normalize_replay_limit(limit: i64) -> i64 {
    if limit <= 0 {
        REPLAY_LIMIT_DEFAULT
    } else {
        limit.clamp(1, REPLAY_LIMIT_MAX)
    }
}

fn to_rfc3339(ts_seconds: f64) -> String {
    let millis = (if ts_seconds.is_finite() && ts_seconds > 0.0 {
        ts_seconds
    } else {
        now_ts()
    } * 1000.0) as i64;
    DateTime::<Utc>::from_timestamp_millis(millis)
        .unwrap_or_else(Utc::now)
        .to_rfc3339()
}

fn map_record_to_event(
    record: Value,
    user_id: &str,
    group_id: &str,
) -> Option<BeeroomProjectionEvent> {
    let event_id = record.get("event_id").and_then(Value::as_i64)?;
    let event_type = record
        .get("event")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let payload = record.get("data").cloned().unwrap_or(Value::Null);
    Some(BeeroomProjectionEvent {
        event_id,
        user_id: user_id.to_string(),
        group_id: group_id.to_string(),
        event_type,
        created_at: resolve_event_created_at(&record, &payload),
        payload,
    })
}

fn resolve_event_created_at(record: &Value, payload: &Value) -> f64 {
    record
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis() as f64 / 1000.0)
        .or_else(|| {
            payload
                .get("created_at")
                .and_then(Value::as_f64)
                .or_else(|| {
                    payload
                        .get("created_at")
                        .and_then(Value::as_i64)
                        .map(|value| value as f64)
                })
        })
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or_else(now_ts)
}

fn chat_message_payload(record: &BeeroomChatMessageRecord) -> Value {
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
    use super::BeeroomProjectionService;
    use crate::services::directory::{RouteLeaseService, RouteTargetKind};
    use crate::storage::{BeeroomChatMessageRecord, SqliteStorage, StorageBackend};
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};

    fn build_service() -> BeeroomProjectionService {
        let db_path = std::env::temp_dir().join(format!(
            "wunder_beeroom_projection_{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let route_leases = Arc::new(RouteLeaseService::new());
        BeeroomProjectionService::new(storage, route_leases)
    }

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
        let service = build_service();
        assert!(service.subscribe_group("", "group-a").await.is_err());
        assert!(service.subscribe_group("user-a", "").await.is_err());
    }

    #[tokio::test]
    async fn subscribe_group_isolated_by_user_and_group() {
        let service = build_service();
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
        let service = build_service();
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
        let service = build_service();
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
    }

    #[tokio::test]
    async fn list_group_events_supports_resume_replay() {
        let service = build_service();
        service
            .publish_group_event("user-a", "group-a", "team_start", json!({ "step": 1 }))
            .await;
        service
            .publish_group_event(
                "user-a",
                "group-a",
                "team_task_update",
                json!({ "step": 2 }),
            )
            .await;

        let replay = service
            .list_group_events("user-a", "group-a", 0, 200)
            .await
            .expect("replay should succeed");
        assert_eq!(replay.len(), 2);
        assert_eq!(replay[0].event_type, "team_start");
        assert_eq!(replay[1].event_type, "team_task_update");

        let replay_after_first = service
            .list_group_events("user-a", "group-a", replay[0].event_id, 200)
            .await
            .expect("incremental replay should succeed");
        assert_eq!(replay_after_first.len(), 1);
        assert_eq!(replay_after_first[0].event_type, "team_task_update");
    }

    #[tokio::test]
    async fn latest_event_id_rejects_empty_identifiers() {
        let service = build_service();
        assert!(service.latest_event_id("", "group-a").await.is_err());
        assert!(service.latest_event_id("user-a", "").await.is_err());
    }

    #[tokio::test]
    async fn lagged_receiver_reports_gap_after_burst_publish() {
        let service = build_service();
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

    #[tokio::test]
    async fn subscribe_group_acquires_projection_route_lease() {
        let db_path = std::env::temp_dir().join(format!(
            "wunder_beeroom_projection_route_{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let route_leases = Arc::new(RouteLeaseService::new());
        let service = BeeroomProjectionService::new(storage, route_leases.clone());

        let _receiver = service
            .subscribe_group("user-a", "group-a")
            .await
            .expect("subscription should succeed");

        let route_target_id =
            BeeroomProjectionService::route_target_id("user-a", "group-a").expect("route key");
        let snapshot = route_leases
            .route_snapshot(RouteTargetKind::Projection, &route_target_id)
            .expect("projection route snapshot should exist");
        assert_eq!(snapshot.user_id.as_deref(), Some("user-a"));
    }
}
