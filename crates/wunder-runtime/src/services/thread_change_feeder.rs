//! Shared completed-history feed backed by the canonical rollout projection.
//!
//! CLI 与桌面 façade 从 durable 帧投影聊天状态；transport `event_id` 不参与
//! 重连、去重或排序。`ThreadChangeHub` 在提交并投影后唤醒订阅者；空闲时不轮询。
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

/// Maximum completed-history frames read in one page.
pub const FEEDER_PAGE_SIZE: i64 = 200;

const CHANNEL_CAPACITY: usize = 256;
/// Durable frame delivered by [`watch_thread_changes`].
#[derive(Debug, Clone)]
pub enum ThreadChangeFrame {
    /// One completed item or turn lifecycle projection (`thread_change`).
    Change {
        /// Exclusive canonical rollout position; cursors may contain gaps.
        seq: i64,
        event: String,
        data: Value,
    },
    /// The rollout/cursor is no longer valid; reload a bounded snapshot.
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
/// `after_seq` is the exclusive canonical rollout position; frames are
/// delivered in strictly increasing cursor order. The returned receiver is the
/// only writer side of this feed; dropping it or cancelling ends the feed.
pub async fn watch_thread_changes(
    state: Arc<AppState>,
    session_id: String,
    after_seq: i64,
    cancel: Option<CancellationToken>,
) -> Result<mpsc::Receiver<ThreadChangeFrame>> {
    let (tx, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    let storage = state.storage.clone();
    // Subscribe before the initial read so a commit during that read remains
    // observable. The write owner publishes only after projection commits.
    let mut hub_notify = state.kernel.orchestrator.change_hub.subscribe(&session_id);
    let mut last_seq = after_seq.max(0);
    let cancel = cancel.unwrap_or_default();

    tokio::spawn(async move {
        loop {
            if cancel.is_cancelled() {
                return;
            }
            let page_session = session_id.clone();
            let snapshot = storage.clone();
            let page = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                _ = tx.closed() => return,
                page = blocking::run_db("thread_change_feeder.read", move || {
                    load_thread_change_frames(snapshot.as_ref(), &page_session, last_seq, FEEDER_PAGE_SIZE)
                }) => page,
            };
            match page {
                Ok(records) => {
                    let mut progressed = false;
                    for record in records {
                        let Some(event) = record.get("event").and_then(Value::as_str) else {
                            continue;
                        };
                        let data = record.get("data").cloned().unwrap_or(Value::Null);
                        if event == "thread_snapshot_required" {
                            let _ = send_frame(
                                &tx,
                                &cancel,
                                ThreadChangeFrame::SnapshotRequired { data },
                            )
                            .await;
                            return;
                        }
                        let seq = data.get("cursor").and_then(Value::as_i64).unwrap_or(0);
                        if seq <= last_seq {
                            continue;
                        }
                        if !send_frame(
                            &tx,
                            &cancel,
                            ThreadChangeFrame::Change {
                                seq,
                                event: event.to_string(),
                                data,
                            },
                        )
                        .await
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
                        // A byte budget can end a page before its row limit.
                        // Drain to an empty page before waiting for another commit.
                        continue;
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        %err,
                        session = %session_id,
                        "thread history subscription read failed"
                    );
                    return;
                }
            }
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                changed = hub_notify.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    // Always re-read on a commit signal: a replacement rollout
                    // can require resetting a cursor rather than increasing it.
                    hub_notify.borrow_and_update();
                }
                _ = tx.closed() => return,
            }
        }
    });
    Ok(receiver)
}

async fn send_frame(
    tx: &mpsc::Sender<ThreadChangeFrame>,
    cancel: &CancellationToken,
    frame: ThreadChangeFrame,
) -> bool {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => false,
        result = tx.send(frame) => result.is_ok(),
    }
}

/// Shared transport projection. Backend implementations resolve this contract
/// against canonical completed history, while the application database keeps
/// only business/control data. All callers share the same cursor guard.
pub(crate) fn load_thread_change_frames(
    storage: &dyn crate::storage::StorageBackend,
    session_id: &str,
    after_seq: i64,
    limit: i64,
) -> Result<Vec<Value>> {
    let after_seq = after_seq.max(0);
    if after_seq > storage.latest_thread_change_seq_by_session(session_id)? {
        return Ok(vec![
            serde_json::json!({"event":"thread_snapshot_required","data":{}}),
        ]);
    }
    let changes = storage.list_thread_changes_by_session(
        session_id,
        after_seq,
        limit.clamp(1, FEEDER_PAGE_SIZE),
    )?;
    let mut frames = Vec::with_capacity(changes.len());
    for mut change in changes {
        if change["change_type"] == "snapshot_required" {
            frames.push(serde_json::json!({"event":"thread_snapshot_required","data":change}));
            break;
        }
        let cursor = change["change_seq"].as_i64().unwrap_or(0);
        if let Some(data) = change.as_object_mut() {
            data.remove("change_seq");
            data.insert("cursor".into(), cursor.into());
        }
        frames.push(serde_json::json!({"event":"thread_change","data":change}));
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config_store::ConfigStore;
    use crate::state::AppStateInitOptions;
    use serde_json::json;
    use wunder_core::thread_store::*;

    async fn build_state(name: &str) -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_dir.path().to_path_buf();
        let mut config = Config::default();
        config.storage.backend = "sqlite".to_string();
        config.storage.data_root = root.join(format!("{name}")).to_string_lossy().to_string();
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
                workspace_id: None,
                tool_overrides: Vec::new(),
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
    }

    async fn seed_canonical(state: &Arc<AppState>, records: Vec<CanonicalRecord>) {
        seed_session(&state.storage);
        state
            .thread_store
            .admission_store()
            .unwrap()
            .register_owner("owner", "feeder-thread")
            .unwrap();
        let thread = ThreadId::new("feeder-thread").unwrap();
        let rollout = RolloutId::new("feeder-rollout").unwrap();
        state
            .thread_store
            .create_thread(CreateThreadRequest {
                metadata: ThreadMetadata {
                    thread_id: thread.clone(),
                    current_rollout_id: rollout.clone(),
                    status: ThreadStatus::Active,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    title: None,
                    workspace_id: None,
                    pinned: false,
                },
                session_meta: SessionMeta {
                    thread_id: thread.clone(),
                    rollout_id: rollout,
                    created_at_ms: 1,
                    source: "test".into(),
                    initial_system_prompt: "sample instruction".into(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: Value::Null,
                },
            })
            .await
            .expect("create canonical thread");
        state
            .thread_store
            .append_canonical(&thread, records)
            .await
            .expect("append fixture");
    }

    fn item(id: &str, kind: CanonicalItemKind) -> CanonicalItem {
        CanonicalItem {
            item_id: ItemId::new(id).unwrap(),
            turn_id: TurnId::new("feeder-turn").unwrap(),
            root_turn_id: None,
            created_at_ms: 10,
            kind,
            payload: json!({"content":"sample"}),
            display_output: None,
            display_output_truncated: false,
        }
    }

    fn started() -> CanonicalRecord {
        CanonicalRecord::Event(CanonicalEvent::TurnStarted {
            turn_id: TurnId::new("feeder-turn").unwrap(),
            root_turn_id: None,
            started_at_ms: 5,
        })
    }

    async fn next_frame(receiver: &mut mpsc::Receiver<ThreadChangeFrame>) -> ThreadChangeFrame {
        tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
            .await
            .expect("frame within timeout")
            .expect("feed remains open")
    }

    #[tokio::test]
    async fn feeder_replays_completed_canonical_events_with_sparse_rollout_positions() {
        let (state, _directory) = build_state("feeder-order").await;
        let user = item("user-item", CanonicalItemKind::UserMessage);
        let answer = item("answer-item", CanonicalItemKind::AssistantMessage);
        seed_canonical(
            &state,
            vec![
                started(),
                CanonicalRecord::ResponseItem(ResponseItem {
                    item_id: user.item_id.clone(),
                    turn_id: user.turn_id.clone(),
                    root_turn_id: None,
                    created_at_ms: 10,
                    kind: user.kind,
                    payload: user.payload.clone(),
                }),
                CanonicalRecord::Event(CanonicalEvent::ItemCompleted(user)),
                CanonicalRecord::ResponseItem(ResponseItem {
                    item_id: answer.item_id.clone(),
                    turn_id: answer.turn_id.clone(),
                    root_turn_id: None,
                    created_at_ms: 11,
                    kind: answer.kind,
                    payload: answer.payload.clone(),
                }),
                CanonicalRecord::Event(CanonicalEvent::ItemCompleted(answer)),
                CanonicalRecord::Event(CanonicalEvent::TurnCompleted {
                    turn_id: TurnId::new("feeder-turn").unwrap(),
                    completed_at_ms: 20,
                }),
            ],
        )
        .await;
        let cancel = CancellationToken::new();
        let mut receiver =
            watch_thread_changes(state, "feeder-thread".into(), 0, Some(cancel.clone()))
                .await
                .expect("start feed");
        let mut positions = Vec::new();
        let mut item_ids = Vec::new();
        for _ in 0..4 {
            let ThreadChangeFrame::Change { seq, data, .. } = next_frame(&mut receiver).await
            else {
                panic!("expected completed history frame");
            };
            positions.push(seq);
            if data["change_type"] == "item_upsert" {
                item_ids.push(data["payload"]["item_id"].as_str().unwrap().to_owned());
                assert_eq!(data["payload"]["payload"]["content"], "sample");
            }
        }
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "{positions:?}"
        );
        assert!(
            positions.windows(2).any(|pair| pair[0] + 1 < pair[1]),
            "context records leave cursor gaps"
        );
        assert_eq!(item_ids, vec!["user-item", "answer-item"]);
        cancel.cancel();
    }

    #[tokio::test]
    async fn feeder_requires_snapshot_for_a_cursor_beyond_the_rollout() {
        let (state, _directory) = build_state("feeder-reset").await;
        seed_canonical(&state, vec![started()]).await;
        let latest = state
            .thread_store
            .history_position(&ThreadId::new("feeder-thread").unwrap())
            .await
            .unwrap()
            .end_ordinal_exclusive as i64;
        let mut receiver = watch_thread_changes(state, "feeder-thread".into(), latest + 1, None)
            .await
            .expect("start feed");
        assert!(matches!(
            next_frame(&mut receiver).await,
            ThreadChangeFrame::SnapshotRequired { .. }
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn idle_feeder_reads_again_only_after_a_commit_wake() {
        let (state, _directory) = build_state("feeder-event-wake").await;
        seed_canonical(&state, vec![started()]).await;
        let thread = ThreadId::new("feeder-thread").unwrap();
        let baseline = state
            .thread_store
            .history_position(&thread)
            .await
            .unwrap()
            .end_ordinal_exclusive as i64;
        let cancel = CancellationToken::new();
        let mut receiver = watch_thread_changes(
            state.clone(),
            "feeder-thread".into(),
            baseline,
            Some(cancel.clone()),
        )
        .await
        .expect("start feed");
        // Let the initial bounded replay finish. No historical frame is pending.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(75), receiver.recv())
                .await
                .is_err()
        );
        let outcome = state
            .thread_store
            .append_canonical(
                &thread,
                vec![CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item(
                    "wake-item",
                    CanonicalItemKind::AssistantMessage,
                )))],
            )
            .await
            .expect("persist and project fixture without publish");
        // The former 250 ms fallback would read this unannounced change. A
        // subscriber now sleeps until the owning producer publishes a commit.
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(350), receiver.recv())
                .await
                .is_err()
        );
        state.kernel.orchestrator.change_hub.publish(
            "feeder-thread",
            outcome.persisted_through.end_ordinal_exclusive as i64,
        );
        let ThreadChangeFrame::Change { data, .. } = next_frame(&mut receiver).await else {
            panic!("expected item");
        };
        assert_eq!(data["payload"]["item_id"], "wake-item");
        cancel.cancel();
    }

    #[tokio::test]
    async fn a_full_output_channel_does_not_delay_cancellation() {
        let (tx, mut receiver) = mpsc::channel(1);
        let frame = ThreadChangeFrame::Overflow {
            cursor: 0,
            resume_recommended: true,
        };
        tx.send(frame.clone()).await.unwrap();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task = tokio::spawn(async move { send_frame(&tx, &task_cancel, frame).await });
        tokio::task::yield_now().await;
        cancel.cancel();
        assert!(
            !tokio::time::timeout(std::time::Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
        );
        assert!(receiver.recv().await.is_some());
        assert!(receiver.recv().await.is_none());
    }

    #[tokio::test]
    async fn feeder_stops_immediately_when_already_cancelled() {
        let (state, _directory) = build_state("feeder-cancel").await;
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut receiver = watch_thread_changes(state, "feeder-thread".into(), 0, Some(cancel))
            .await
            .expect("start feed");
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), receiver.recv())
                .await
                .unwrap()
                .is_none()
        );
    }
}
