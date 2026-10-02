//! Passive, bounded observation of every user turn in the selected thread.
use super::{chat_turns, NativeChatTurn, NativeDesktop};
use anyhow::{anyhow, Result};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wunder_server::{blocking, state::AppState, watch_thread_changes, ThreadChangeFrame};

#[derive(Debug)]
pub enum NativeThreadUpdate {
    Reset(Vec<NativeChatTurn>),
    Turns(Vec<NativeChatTurn>),
    Failed(String),
}

pub struct NativeThreadWatch {
    receiver: mpsc::Receiver<NativeThreadUpdate>,
    cancel: CancellationToken,
}

impl Drop for NativeThreadWatch {
    fn drop(&mut self) {
        // Unsubscribing must never interrupt a channel or background execution.
        self.cancel.cancel();
    }
}

impl NativeThreadWatch {
    pub fn try_recv(&mut self) -> Option<NativeThreadUpdate> {
        self.receiver.try_recv().ok()
    }
}

impl NativeDesktop {
    pub fn watch_chat(&self, session: &str) -> NativeThreadWatch {
        let (output, receiver) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let state = self.state().clone();
        let user = self.user_id().to_owned();
        let session = session.to_owned();
        self.runtime.spawn(async move {
            tokio::select! {
                _ = token.cancelled() => {},
                result = observe(state, user, session, &output, &token) => {
                    if let Err(error) = result {
                        tracing::warn!(%error, "native thread observation failed");
                        let _ = output.send(NativeThreadUpdate::Failed("线程更新失败，请重新选择线程".into())).await;
                    }
                }
            }
        });
        NativeThreadWatch { receiver, cancel }
    }
}

async fn observe(
    state: Arc<AppState>,
    user: String,
    session: String,
    output: &mpsc::Sender<NativeThreadUpdate>,
    cancel: &CancellationToken,
) -> Result<()> {
    loop {
        let db = state.clone();
        let owner = user.clone();
        let target = session.clone();
        let (cursor, turns) = blocking::run_db("native.observe.initial", move || {
            db.user_store
                .get_chat_session(&owner, &target)?
                .ok_or_else(|| anyhow!("chat session not found"))?;
            // Read the watermark BEFORE history. Concurrent writes are replayed;
            // stable root identities make those repetitions harmless.
            let cursor = db.storage.latest_thread_change_seq_by_session(&target)?;
            Ok((cursor, chat_turns::load_turns(&db, &owner, &target)?))
        })
        .await?;
        if output.send(NativeThreadUpdate::Reset(turns)).await.is_err() {
            return Ok(());
        }
        let feed_cancel = cancel.child_token();
        let _feed_guard = feed_cancel.clone().drop_guard();
        let mut feed =
            watch_thread_changes(state.clone(), session.clone(), cursor, Some(feed_cancel)).await?;
        let mut reset = false;
        while let Some(first) = feed.recv().await {
            let mut executions = BTreeSet::new();
            let mut collect = |frame: ThreadChangeFrame| match frame {
                ThreadChangeFrame::Change { data, .. } => {
                    if let Some(id) = data["turn_id"].as_str().filter(|id| !id.is_empty()) {
                        executions.insert(id.to_owned());
                    }
                }
                _ => reset = true,
            };
            collect(first);
            // Merge durable commits from a burst, with bounded feed and output.
            tokio::time::sleep(Duration::from_millis(80)).await;
            for _ in 0..255 {
                match feed.try_recv() {
                    Ok(frame) => collect(frame),
                    Err(_) => break,
                }
            }
            if reset {
                break;
            }
            let db = state.clone();
            let owner = user.clone();
            let target = session.clone();
            let turns = blocking::run_db("native.observe.turns", move || {
                let mut roots = BTreeSet::new();
                let mut turns = Vec::new();
                for execution in executions {
                    if let Some(detail) = db
                        .storage
                        .get_thread_turn(&owner, &target, &execution, -1, 1, false)?
                    {
                        roots.insert(
                            detail["root_turn_id"]
                                .as_str()
                                .unwrap_or(&execution)
                                .to_owned(),
                        );
                    }
                }
                // Preserve user-round ordering even when several channels write
                // during the same frame. No historical message reconstruction.
                let mut records = Vec::new();
                for root in roots {
                    if let Some(detail) = db
                        .storage
                        .get_thread_turn(&owner, &target, &root, -1, 1, false)?
                    {
                        records.push(detail);
                    }
                }
                records.sort_by_key(|record| record["user_turn_index"].as_i64().unwrap_or(0));
                for root in records {
                    if let Some(turn) = chat_turns::load_turn(&db, &owner, &target, &root)? {
                        turns.push(turn);
                    }
                }
                Ok(turns)
            })
            .await?;
            if !turns.is_empty() && output.send(NativeThreadUpdate::Turns(turns)).await.is_err() {
                return Ok(());
            }
        }
        if cancel.is_cancelled() || output.is_closed() {
            return Ok(());
        }
        if !reset {
            return Err(anyhow!("native durable subscription closed"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wunder_server::{config::Config, config_store::ConfigStore, state::AppStateInitOptions};

    async fn next(receiver: &mut mpsc::Receiver<NativeThreadUpdate>) -> Vec<NativeChatTurn> {
        match tokio::time::timeout(Duration::from_secs(4), receiver.recv())
            .await
            .unwrap()
            .unwrap()
        {
            NativeThreadUpdate::Reset(turns) | NativeThreadUpdate::Turns(turns) => turns,
            NativeThreadUpdate::Failed(error) => panic!("{error}"),
        }
    }

    #[tokio::test]
    async fn channel_turns_arrive_without_local_send_and_survive_resubscription() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.db_path = temp
            .path()
            .join("fixture.db")
            .to_string_lossy()
            .into_owned();
        config.workspace.root = temp.path().join("workspace").to_string_lossy().into_owned();
        config.skills.enabled.clear();
        let store = ConfigStore::new(temp.path().join("fixture.yaml"));
        store.update(|value| *value = config.clone()).await.unwrap();
        let state = Arc::new(
            AppState::new_with_options(
                store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        let user = "fixture-owner";
        let session = "fixture-channel-thread";
        state
            .user_store
            .upsert_chat_session(&wunder_server::storage::ChatSessionRecord {
                session_id: session.into(),
                user_id: user.into(),
                title: "Fixture".into(),
                status: "active".into(),
                created_at: 0.0,
                updated_at: 0.0,
                last_message_at: 0.0,
                agent_id: None,
                tool_overrides: vec![],
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
        let (output, mut receiver) = mpsc::channel(8);
        let cancel = CancellationToken::new();
        let db = state.clone();
        let token = cancel.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => Ok(()),
                result = observe(db, user.into(), session.into(), &output, &token) => result,
            }
        });
        assert!(next(&mut receiver).await.is_empty());
        let mut ids = Vec::new();
        for input in ["Fixture input", "/help", "[attachment]"] {
            let accepted = state
                .storage
                .accept_thread_turn(user, session, &json!({"content": input}))
                .unwrap();
            let id = accepted["turn_id"].as_str().unwrap().to_string();
            ids.push(id.clone());
            let rows = next(&mut receiver).await;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].root_id, id);
            assert_eq!(rows[0].user.text, input);
            assert!(rows[0].assistant.text.is_empty());
            let answer = format!("Fixture result {}", ids.len());
            state
                .storage
                .append_thread_item(
                    user,
                    &json!({"session_id":session,"turn_id":id,
                "item_id":format!("{id}:text-0"),"kind":"assistant_message","role":"assistant",
                "model_round":0,"content":answer,"status":"completed"}),
                )
                .unwrap();
            state
                .storage
                .update_thread_turn(user, session, &id, "completed", "", &json!({}))
                .unwrap();
            loop {
                let rows = next(&mut receiver).await;
                if rows[0].assistant.stats_status == "任务完成" {
                    assert_eq!(rows[0].assistant.text, answer);
                    break;
                }
            }
            assert!(
                !task.is_finished(),
                "completion must not end thread observation"
            );
        }
        let accepted = state
            .storage
            .accept_thread_turn(user, session, &json!({"content":"Fixture pending"}))
            .unwrap();
        let id = accepted["turn_id"].as_str().unwrap();
        let item = format!("{id}:text-1");
        state
            .storage
            .append_thread_item(
                user,
                &json!({"session_id":session,"turn_id":id,
            "item_id":item,"kind":"assistant_message","role":"assistant","model_round":1,
            "content":"","status":"running"}),
            )
            .unwrap();
        state.storage.upsert_thread_text_block(user, session, &json!({"item_id":item,"field":"content",
            "block_index":0,"event_id":1,"data":{"field":"content","content_offset":0,"content":"Fixture partial"}})).unwrap();
        loop {
            if next(&mut receiver)
                .await
                .iter()
                .any(|turn| turn.assistant.text == "Fixture partial")
            {
                break;
            }
        }
        // A UI detach cancels observation only, preserving the pending execution.
        let watch = NativeThreadWatch { receiver, cancel };
        drop(watch);
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let detail = state
            .storage
            .get_thread_turn(user, session, id, -1, 10, false)
            .unwrap()
            .unwrap();
        assert_eq!(detail["status"], "queued");
        state
            .storage
            .update_thread_turn(user, session, id, "cancelled", "", &json!({}))
            .unwrap();
        let (output, mut receiver) = mpsc::channel(8);
        let db = state.clone();
        let task = tokio::spawn(async move {
            observe(
                db,
                user.into(),
                session.into(),
                &output,
                &CancellationToken::new(),
            )
            .await
        });
        let restored = next(&mut receiver).await;
        assert_eq!(restored.len(), 4);
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(&restored[index].root_id, id);
        }
        assert_eq!(restored[3].assistant.text, "Fixture partial");
        assert_eq!(restored[3].assistant.stats_status, "已停止");
        task.abort();
        // The initial read validates ownership, before exposing any history.
        let (output, _receiver) = mpsc::channel(8);
        assert!(observe(
            state,
            "fixture-other".into(),
            session.into(),
            &output,
            &CancellationToken::new()
        )
        .await
        .is_err());
    }
}
