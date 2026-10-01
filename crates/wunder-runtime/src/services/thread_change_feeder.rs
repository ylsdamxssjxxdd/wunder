//! Shared durable ThreadLog change feeder (聊天流式管线根治方案 §3.3/§3.4，M1-C 消费侧共享层).
//!
//! CLI 与桌面 façade 从 durable 帧投影聊天状态；transport `event_id` 不参与
//! 重连、去重或排序。`ThreadChangeHub` 只是唤醒信号，轮询兜底保证跨实例正确；
//! 消费方在会话切换、停止监视或队列水位时通过 `CancellationToken` 结束 feeder。
//!
//! feeder 只产出 durable 帧；ephemeral `thread_item_tail` 由各自的实时通道处理，
//! 不在此处混流。页大小按根治方案固定为 200，持续读满页后立即继续读取。

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::core::blocking;
use crate::state::AppState;

/// One durable change page per poll (根治方案 §3.3：feeder 每页 200).
pub const FEEDER_PAGE_SIZE: i64 = 200;

const CHANNEL_CAPACITY: usize = 256;
const BASE_POLL_INTERVAL_MS: u64 = 250;
const MAX_POLL_INTERVAL_MS: u64 = 4000;
const BACKOFF_FACTOR: f64 = 2.0;

/// Durable frame delivered by [`watch_thread_changes`].
#[derive(Debug, Clone)]
pub enum ThreadChangeFrame {
    /// One durable change row (`thread_change` / `thread_item_block`).
    Change {
        /// `change_seq` of this frame; the durable cursor.
        seq: i64,
        event: String,
        data: Value,
    },
    /// The durable window was trimmed; consumer must reload an atomic snapshot.
    SnapshotRequired { data: Value },
    /// The consumer fell behind; reconnect from `cursor` (control frame, not a change).
    Overflow {
        cursor: i64,
        resume_recommended: bool,
    },
}

impl ThreadChangeFrame {
    /// Durable cursor of this frame (0 for control frames).
    pub fn seq(&self) -> i64 {
        match self {
            ThreadChangeFrame::Change { seq, .. } => *seq,
            ThreadChangeFrame::SnapshotRequired { .. } | ThreadChangeFrame::Overflow { .. } => 0,
        }
    }

    pub fn is_control(&self) -> bool {
        !matches!(self, ThreadChangeFrame::Change { .. })
    }
}

/// Start a durable change feed for one session.
///
/// `after_seq` is the exclusive durable cursor (`change_seq`); frames are
/// delivered in strictly increasing cursor order. The returned receiver is the
/// only writer side of this feed; dropping it or cancelling ends the feed.
pub async fn watch_thread_changes(
    state: Arc<AppState>,
    session_id: String,
    after_seq: i64,
    cancel: Option<CancellationToken>,
) -> Result<mpsc::Receiver<ThreadChangeFrame>> {
    let (tx, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let workspace = state.workspace.clone();
    // The hub is only a wake signal; the poll fallback stays the correctness
    // path, so a feed without subscribers is never treated as truth.
    let hub_notify = state.kernel.orchestrator.change_hub.subscribe(&session_id);
    let mut last_seq = after_seq.max(0);
    let mut idle_rounds = 0usize;
    let mut poll_interval = BASE_POLL_INTERVAL_MS;

    tokio::spawn(async move {
        loop {
            if cancel.as_ref().is_some_and(|token| token.is_cancelled()) {
                return;
            }
            let page_session = session_id.clone();
            let snapshot = workspace.clone();
            let page = blocking::run_fs("thread_change_feeder.read", move || {
                snapshot.try_load_thread_changes(&page_session, last_seq, FEEDER_PAGE_SIZE)
            })
            .await;
            match page {
                Ok(records) => {
                    let full_page = records.len() as i64 == FEEDER_PAGE_SIZE;
                    let mut progressed = false;
                    for record in records {
                        let Some(event) = record.get("event").and_then(Value::as_str) else {
                            continue;
                        };
                        let data = record.get("data").cloned().unwrap_or(Value::Null);
                        if event == "thread_snapshot_required" {
                            let _ = tx.send(ThreadChangeFrame::SnapshotRequired { data }).await;
                            return;
                        }
                        let seq = data.get("cursor").and_then(Value::as_i64).unwrap_or(0);
                        if tx
                            .send(ThreadChangeFrame::Change {
                                seq,
                                event: event.to_string(),
                                data,
                            })
                            .await
                            .is_err()
                        {
                            // Consumer went away: stop the feed.
                            return;
                        }
                        if seq > last_seq {
                            last_seq = seq;
                            progressed = true;
                        }
                    }
                    if progressed {
                        idle_rounds = 0;
                        poll_interval = BASE_POLL_INTERVAL_MS;
                        if full_page {
                            // More durable changes likely wait; re-read immediately.
                            continue;
                        }
                    } else {
                        idle_rounds = idle_rounds.saturating_add(1);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        %err,
                        session = %session_id,
                        "thread change feeder read failed; backing off"
                    );
                    idle_rounds = idle_rounds.saturating_add(1);
                }
            }
            if idle_rounds > 0 {
                let backoff = (BASE_POLL_INTERVAL_MS as f64
                    * BACKOFF_FACTOR.powi(idle_rounds.min(6) as i32))
                    as u64;
                poll_interval = poll_interval.min(backoff.min(MAX_POLL_INTERVAL_MS));
            } else {
                poll_interval = BASE_POLL_INTERVAL_MS;
            }
            let wait = tokio::time::sleep(std::time::Duration::from_millis(poll_interval));
            let wake = async {
                let mut notify = hub_notify.clone();
                let _ = notify.changed().await;
            };
            let cancel_fut: std::pin::Pin<
                std::boxed::Box<dyn std::future::Future<Output = ()> + Send>,
            > = match cancel.as_ref() {
                Some(token) => std::boxed::Box::pin(token.cancelled()),
                None => std::boxed::Box::pin(std::future::pending()),
            };
            tokio::select! {
                _ = cancel_fut => return,
                _ = wake => {
                    // Wake signal only: re-read when the hub claims a cursor
                    // ahead of our durable position.
                    if *hub_notify.borrow() > last_seq {
                        idle_rounds = 0;
                        poll_interval = BASE_POLL_INTERVAL_MS;
                    }
                }
                _ = wait => {}
            }
        }
    });
    Ok(receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config_store::ConfigStore;
    use crate::state::AppStateInitOptions;
    use serde_json::json;

    async fn build_state(name: &str) -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_dir.path().to_path_buf();
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.db_path = root
            .join(format!("{name}.db"))
            .to_string_lossy()
            .to_string();
        config.workspace.root = root.join("workspaces").to_string_lossy().to_string();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(root.join("wunder.yaml"));
        let config_for_store = config.clone();
        config_store
            .update(|current| *current = config_for_store.clone())
            .await
            .expect("write config");
        let state = Arc::new(
            AppState::new_with_options(
                config_store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .expect("create app state"),
        );
        (state, temp_dir)
    }

    fn seed_session(storage: &Arc<dyn crate::storage::StorageBackend>) {
        storage
            .upsert_chat_session(&crate::storage::ChatSessionRecord {
                session_id: "feeder-thread".to_string(),
                user_id: "owner".to_string(),
                title: "T".to_string(),
                status: "active".to_string(),
                created_at: 1.0,
                updated_at: 1.0,
                last_message_at: 1.0,
                agent_id: None,
                tool_overrides: Vec::new(),
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
    }

    fn seed_answer_item(state: &Arc<AppState>, turn: &Value) {
        state
            .storage
            .append_thread_item(
                "owner",
                &json!({"session_id":"feeder-thread",
                    "turn_id":turn["turn_id"], "item_id":"answer", "kind":"assistant_message",
                    "role":"assistant", "status":"running", "visibility":"user"}),
            )
            .unwrap();
    }

    #[tokio::test]
    async fn feeder_delivers_durable_frames_in_change_seq_order() {
        let (state, _temp_dir) = build_state("feeder-order").await;
        seed_session(&state.storage);
        let turn = state
            .storage
            .accept_thread_turn("owner", "feeder-thread", &json!({"content":"hi"}))
            .unwrap();
        seed_answer_item(&state, &turn);
        state
            .storage
            .upsert_thread_text_block(
                "owner",
                "feeder-thread",
                &json!({"item_id":"answer", "field":"content", "block_index":0, "event_id":1000,
                    "data":{"content":"hello", "field":"content", "block_index":0, "content_offset":0}}),
            )
            .unwrap();

        let cancel = CancellationToken::new();
        let mut rx = watch_thread_changes(
            state.clone(),
            "feeder-thread".into(),
            0,
            Some(cancel.clone()),
        )
        .await
        .expect("start feeder");

        let mut seqs = Vec::new();
        for _ in 0..8 {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
                .await
                .expect("frame within timeout")
                .expect("receiver open");
            match &frame {
                ThreadChangeFrame::Change { seq, .. } => seqs.push(*seq),
                ThreadChangeFrame::SnapshotRequired { .. } => break,
                ThreadChangeFrame::Overflow { .. } => break,
            }
            if seqs.len() >= 3 {
                break;
            }
        }
        // One immutable durable frame per change_seq, including text blocks.
        assert!(
            seqs.windows(2).all(|pair| pair[0] + 1 == pair[1]),
            "seqs: {seqs:?}"
        );
        assert!(seqs.len() >= 3, "seqs: {seqs:?}");
        cancel.cancel();
    }

    #[tokio::test]
    async fn feeder_stops_on_snapshot_required_when_cursor_is_beyond_window() {
        let (state, _temp_dir) = build_state("feeder-snapshot").await;
        seed_session(&state.storage);
        let turn = state
            .storage
            .accept_thread_turn("owner", "feeder-thread", &json!({"content":"hi"}))
            .unwrap();
        seed_answer_item(&state, &turn);
        let latest = state
            .workspace
            .latest_thread_change_seq("feeder-thread")
            .unwrap();

        let mut rx = watch_thread_changes(state.clone(), "feeder-thread".into(), latest + 1, None)
            .await
            .expect("start feeder");
        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("frame within timeout")
            .expect("receiver open");
        assert!(matches!(frame, ThreadChangeFrame::SnapshotRequired { .. }));
        // The feeder returns right after snapshot-required: the channel closes
        // and recv yields None immediately.
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("recv must resolve once the feeder stops");
        assert!(second.is_none(), "feeder must stop after snapshot-required");
    }

    #[tokio::test]
    async fn feeder_stops_when_cancelled() {
        let (state, _temp_dir) = build_state("feeder-cancel").await;
        seed_session(&state.storage);
        let cancel = CancellationToken::new();
        let cancel_for_spawn = cancel.clone();
        let mut rx = tokio::select! {
            rx = watch_thread_changes(state.clone(), "feeder-thread".into(), 0, Some(cancel_for_spawn)) => {
                rx.expect("start feeder")
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                panic!("feeder started within timeout");
            }
        };
        cancel.cancel();
        let stopped = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("recv must resolve once the feeder stops");
        assert!(stopped.is_none(), "cancelled feeder must stop");
    }
}
