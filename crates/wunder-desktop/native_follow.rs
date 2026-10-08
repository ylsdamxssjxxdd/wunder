//! Subscribe to background executions by durable cursor, scoped to one user turn.
use super::{cancel_chat, NativeChatEvent};
use anyhow::{anyhow, ensure, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wunder_server::{blocking, state::AppState, watch_thread_changes, ThreadChangeFrame};

#[derive(Default)]
struct Projection {
    root: String,
    execution: String,
    status: String,
    cursor: i64,
    // Only the active execution is retained; late records from prior executions
    // cannot reset its model-round clock or overwrite its answer.
    items: HashMap<String, (i64, String, i64)>,
    text_item: String,
    text: String,
    offset: usize,
}

impl Projection {
    fn restore(&mut self, snapshot: &Value) -> Result<Vec<Value>> {
        let cursor = snapshot["cursor"]
            .as_i64()
            .ok_or_else(|| anyhow!("snapshot cursor missing"))?;
        let turns = snapshot["turns"]
            .as_array()
            .ok_or_else(|| anyhow!("snapshot turns missing"))?;
        let latest = turns
            .iter()
            .rev()
            .find(|turn| turn["root_turn_id"] == self.root || turn["turn_id"] == self.root)
            .ok_or_else(|| anyhow!("snapshot root missing"))?;
        let execution = latest["turn_id"]
            .as_str()
            .ok_or_else(|| anyhow!("snapshot execution missing"))?;
        self.execution.clear();
        self.items.clear();
        self.text.clear();
        self.text_item.clear();
        self.offset = 0;
        self.cursor = 0;
        let mut events = self.apply(
            &json!({"cursor":1,"change_type":"turn_upsert","turn_id":execution,
            "payload":{"root_turn_id":self.root,"status":"running"}}),
        )?;
        if let Some(items) = snapshot["items"].as_array() {
            for item in items.iter().filter(|item| item["turn_id"] == execution) {
                events.extend(self.apply(&json!({"cursor":self.cursor+1,"change_type":"item_upsert","turn_id":execution,"payload":item}))?);
            }
        }
        if let Some(blocks) = snapshot["blocks"].as_array() {
            for block in blocks
                .iter()
                .filter(|block| block["item_id"] == self.text_item)
                .cloned()
                .collect::<Vec<_>>()
            {
                events.extend(self.apply(
                    &json!({"cursor":self.cursor+1,"change_type":"text_block","turn_id":execution,
                    "item_id":block["item_id"],"payload":block.get("data").unwrap_or(&block)}),
                )?);
            }
        }
        self.status = latest["status"].as_str().unwrap_or_default().into();
        self.cursor = cursor;
        events.push(json!({"event":"native_execution_status","data":{"status":self.status}}));
        Ok(events)
    }

    fn apply(&mut self, data: &Value) -> Result<Vec<Value>> {
        let seq = data["cursor"].as_i64().unwrap_or(0);
        if seq <= self.cursor {
            return Ok(vec![]);
        }
        ensure!(seq == self.cursor + 1, "native durable cursor gap");
        self.cursor = seq;
        let p = &data["payload"];
        let execution = data["turn_id"].as_str().unwrap_or_default();
        let mut events = Vec::new();
        match data["change_type"].as_str().unwrap_or_default() {
            "turn_upsert" | "turn_status" => {
                let root = p["root_turn_id"].as_str().unwrap_or(execution);
                if root != self.root {
                    return Ok(events);
                }
                if self.execution != execution {
                    // Acceptance starts an execution. Status changes for old
                    // executions must never switch the active execution back.
                    if !matches!(p["status"].as_str(), Some("queued" | "running"))
                        || (!self.execution.is_empty()
                            && data["revision"].as_i64().unwrap_or(1) != 1)
                    {
                        return Ok(events);
                    }
                    self.execution = execution.into();
                    self.items.clear();
                    self.text_item.clear();
                    self.text.clear();
                    self.offset = 0;
                    events.push(json!({"event":"native_execution_started","data":{"root_turn_id":self.root,"turn_id":execution}}));
                }
                self.status = p["status"].as_str().unwrap_or_default().into();
                events
                    .push(json!({"event":"native_execution_status","data":{"status":self.status}}));
            }
            "item_upsert" if execution == self.execution => {
                if matches!(p["visibility"].as_str(), Some("admin" | "model_internal")) {
                    return Ok(events);
                }
                let payload = p.get("payload").unwrap_or(p);
                let id = p["item_id"]
                    .as_str()
                    .or_else(|| data["item_id"].as_str())
                    .unwrap_or_default();
                let revision = p["revision"]
                    .as_i64()
                    .or_else(|| data["revision"].as_i64())
                    .unwrap_or(0);
                if self.items.get(id).is_some_and(|entry| entry.0 >= revision) {
                    return Ok(events);
                }
                ensure!(
                    self.items.contains_key(id) || self.items.len() < 2048,
                    "native execution item limit exceeded"
                );
                let kind = p["kind"].as_str().unwrap_or_default();
                let round = payload["model_round"].as_i64().unwrap_or(1);
                self.items.insert(id.into(), (revision, kind.into(), round));
                if kind == "assistant_message" && id == format!("{execution}:text-{round}") {
                    if self
                        .items
                        .get(&self.text_item)
                        .is_some_and(|entry| entry.2 > round)
                    {
                        return Ok(events);
                    }
                    if self.text_item != id {
                        self.text_item = id.into();
                        self.text.clear();
                        self.offset = 0;
                    }
                    if let Some(content) =
                        payload["content"].as_str().filter(|text| !text.is_empty())
                    {
                        ensure!(
                            content.len() <= 8 * 1024 * 1024,
                            "native text limit exceeded"
                        );
                        self.text = content.into();
                        self.offset = content.encode_utf16().count();
                        events.push(json!({"event":"llm_output","data":{"content":content,"model_round":round}}));
                    }
                    if let Some(stats) = payload
                        .pointer("/meta/message_stats")
                        .or_else(|| payload.get("stats"))
                    {
                        events.push(json!({"event":"native_stats","data":stats}));
                    }
                } else if kind == "tool_call" {
                    let pending =
                        matches!(p["status"].as_str(), Some("running" | "queued" | "pending"));
                    let tool = payload["tool"]
                        .as_str()
                        .or_else(|| payload["tool_name"].as_str())
                        .or_else(|| payload["name"].as_str())
                        .unwrap_or("工具");
                    let mut tool_data = payload.clone();
                    tool_data["item_id"] = json!(id);
                    events.push(json!({"event":if pending {"tool_call"} else {"tool_result"}, "data":tool_data,
                        "display_result":wunder_server::tool_result_display::tool_result_display(tool, payload, pending)}));
                } else if kind == "compaction" {
                    let mut detail = payload.clone();
                    detail["status"] = p["status"].clone();
                    detail["item_id"] = json!(id);
                    events.push(json!({"event":"compaction","data":detail}));
                } else if kind == "queue" && p["status"] == "queued" {
                    events.push(json!({"event":"queue_update","data":payload}));
                }
            }
            "text_block" if execution == self.execution => {
                let id = data["item_id"].as_str().unwrap_or_default();
                if id != self.text_item || p["field"].as_str().unwrap_or("content") != "content" {
                    return Ok(events);
                }
                let block = p.get("data").unwrap_or(p);
                let offset = block["content_offset"].as_u64().unwrap_or(0) as usize;
                let text = block["content"].as_str().unwrap_or_default();
                let end = offset + text.encode_utf16().count();
                if end <= self.offset {
                    return Ok(events);
                }
                // The emitter re-flushes the active tail block in place: the
                // same block_index returns with an identical start offset and
                // longer text under a new change_seq. The durable block is
                // authoritative, so the applied text is grown from the block
                // start and the UI only receives the newly added suffix.
                let delta = if offset < self.offset {
                    let skip = self.offset - offset;
                    let split = utf16_byte_index(text, skip)
                        .ok_or_else(|| anyhow!("native text block rewrite boundary"))?;
                    ensure!(
                        self.text.ends_with(&text[..split]),
                        "native text block rewrite mismatch"
                    );
                    self.text.truncate(self.text.len() - text[..split].len());
                    &text[split..]
                } else {
                    ensure!(offset == self.offset, "native text block gap");
                    text
                };
                ensure!(
                    self.text.len() + text.len() <= 8 * 1024 * 1024,
                    "native text limit exceeded"
                );
                self.text.push_str(text);
                self.offset = end;
                let round = self.items.get(id).map(|v| v.2).unwrap_or(1);
                events.push(
                    json!({"event":"llm_output_delta","data":{"delta":delta,"model_round":round}}),
                );
            }
            _ => {}
        }
        Ok(events)
    }

    fn settled(&self, goal_active: bool) -> bool {
        matches!(
            self.status.as_str(),
            "failed" | "cancelled" | "interrupted" | "waiting_input" | "waiting_user_input"
        ) || (self.status == "completed" && !goal_active)
    }
}

/// Byte offset of the given UTF-16 code-unit index inside `text`, or `None`
/// when the index falls inside a multi-unit character (surrogate pair).
fn utf16_byte_index(text: &str, units: usize) -> Option<usize> {
    let mut walked = 0usize;
    for (index, ch) in text.char_indices() {
        if walked == units {
            return Some(index);
        }
        walked += ch.len_utf16();
        if walked > units {
            return None;
        }
    }
    (walked == units).then_some(text.len())
}

pub(super) async fn watch(
    state: &Arc<AppState>,
    user: &str,
    session: &str,
    root: &str,
    cursor: i64,
    output: &mpsc::Sender<NativeChatEvent>,
    cancel: &CancellationToken,
    continuing: bool,
) -> Result<()> {
    ensure!(!root.is_empty(), "missing native root turn identity");
    let feed_cancel = CancellationToken::new();
    let _guard = feed_cancel.clone().drop_guard();
    let mut feed = watch_thread_changes(
        state.clone(),
        session.into(),
        cursor,
        Some(feed_cancel.clone()),
    )
    .await?;
    let mut projection = Projection {
        root: root.into(),
        cursor,
        ..Default::default()
    };
    if continuing {
        projection.execution = root.into();
        projection.status = "completed".into();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                cancel_chat(state, user, session).await?;
                return Ok(());
            }
            result = output.send(NativeChatEvent::Event(
                json!({"event":"native_execution_status","data":{"status":"continuing"}}),
            )) => { if result.is_err() { return Ok(()); } }
        }
    }
    let mut clock = tokio::time::interval(Duration::from_millis(100));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                cancel_chat(state, user, session).await?;
                // The UI has already marked this turn stopped. A full delivery
                // buffer must never prevent backend cancellation from settling.
                let _ = output.try_send(NativeChatEvent::Event(json!({"event":"turn_terminal","data":{"status":"cancelled"}})));
                return Ok(());
            }
            _ = output.closed() => return Ok(()),
            frame = feed.recv() => {
                match frame.ok_or_else(|| anyhow!("native durable subscription closed"))? {
                    ThreadChangeFrame::Change { data, .. } => {
                        for event in projection.apply(&data)? {
                            tokio::select! {
                                _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(()); }
                                result = output.send(NativeChatEvent::Event(event)) => { if result.is_err() { return Ok(()); } }
                            }
                        }
                    }
                    ThreadChangeFrame::SnapshotRequired { .. } => {
                        let storage = state.storage.clone();
                        let owner = user.to_string(); let target = session.to_string();
                        let snapshot = blocking::run_db("native.follow.snapshot", move || storage.thread_snapshot(&owner, &target)).await?;
                        for event in projection.restore(&snapshot)? {
                            tokio::select! {
                                _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(()); }
                                result = output.send(NativeChatEvent::Event(event)) => { if result.is_err() { return Ok(()); } }
                            }
                        }
                        feed = watch_thread_changes(state.clone(), session.into(), projection.cursor, Some(feed_cancel.clone())).await?;
                    }
                    ThreadChangeFrame::Overflow { .. } => {
                        feed = watch_thread_changes(state.clone(), session.into(), projection.cursor, Some(feed_cancel.clone())).await?;
                    }
                }
            }
            _ = clock.tick() => {
                let storage = state.storage.clone();
                let owner = user.to_string(); let target = session.to_string();
                let (active, watermark) = blocking::run_db("native.follow.settlement", move || {
                    let goal = storage.get_session_goal(&owner, &target)?;
                    let active = goal.as_ref().is_some_and(|goal| goal.phase == "active");
                    Ok((active, storage.latest_thread_change_seq_by_session(&target)?))
                }).await?;
                // Drain the terminal item's text/statistics commits before Finished.
                if projection.settled(active) && projection.cursor >= watermark {
                    let status = projection.status.clone();
                    output.send(NativeChatEvent::Event(json!({"event":"turn_terminal","data":{"status":status}}))).await.ok();
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuation_resets_execution_not_root_and_waits_for_goal() {
        let mut p = Projection {
            root: "fixture-root".into(),
            ..Default::default()
        };
        for (seq, execution, status) in [
            (1, "fixture-root", "queued"),
            (2, "fixture-root", "completed"),
            (3, "fixture-child", "queued"),
        ] {
            p.apply(&json!({"cursor":seq,"change_type":"turn_upsert","turn_id":execution,"payload":{"root_turn_id":"fixture-root","status":status}})).unwrap();
        }
        assert_eq!(p.root, "fixture-root");
        assert_eq!(p.execution, "fixture-child");
        p.apply(&json!({"cursor":4,"revision":9,"change_type":"turn_upsert","turn_id":"fixture-root","payload":{"root_turn_id":"fixture-root","status":"running"}})).unwrap();
        assert_eq!(p.execution, "fixture-child");
        p.status = "completed".into();
        assert!(!p.settled(true));
        assert!(p.settled(false));
        p.status = "cancelled".into();
        assert!(p.settled(true));
    }
    #[test]
    fn durable_blocks_duplicate_replay_and_foreign_turn_are_isolated() {
        let mut p = Projection {
            root: "fixture-root".into(),
            execution: "fixture-child".into(),
            ..Default::default()
        };
        let item = json!({"cursor":1,"change_type":"item_upsert","turn_id":"fixture-child","payload":{"item_id":"fixture-child:text-1","kind":"assistant_message","revision":1,"payload":{"content":"","model_round":1}}});
        p.apply(&item).unwrap();
        assert!(p.apply(&item).unwrap().is_empty());
        let block = json!({"cursor":2,"change_type":"text_block","turn_id":"fixture-child","item_id":"fixture-child:text-1","payload":{"content":"🐣","content_offset":0,"field":"content"}});
        assert_eq!(p.apply(&block).unwrap().len(), 1);
        assert!(p.apply(&block).unwrap().is_empty());
        assert_eq!(p.offset, 2);
        p.apply(&json!({"cursor":3,"change_type":"turn_upsert","turn_id":"other","payload":{"status":"queued"}})).unwrap();
        assert_eq!(p.execution, "fixture-child");
        assert_eq!(p.text, "🐣");
    }

    #[test]
    fn tail_block_rewrite_extends_text_and_streams_only_suffix() {
        let mut p = Projection {
            root: "fixture-root".into(),
            execution: "fixture-child".into(),
            ..Default::default()
        };
        let item = json!({"cursor":1,"change_type":"item_upsert","turn_id":"fixture-child","payload":{"item_id":"fixture-child:text-1","kind":"assistant_message","revision":1,"payload":{"content":"","model_round":1}}});
        p.apply(&item).unwrap();
        // First due-flush of the active tail block.
        let short = json!({"cursor":2,"change_type":"text_block","turn_id":"fixture-child","item_id":"fixture-child:text-1","payload":{"content":"你好，","content_offset":0,"field":"content"}});
        let events = p.apply(&short).unwrap();
        assert_eq!(events[0]["data"]["delta"], "你好，");
        // The emitter re-flushes the same block_index with longer text under a
        // new change_seq; the suffix is streamed and the text stays identical
        // to the durable block content.
        let longer = json!({"cursor":3,"change_type":"text_block","turn_id":"fixture-child","item_id":"fixture-child:text-1","payload":{"content":"你好，世界","content_offset":0,"field":"content"}});
        let events = p.apply(&longer).unwrap();
        assert_eq!(events[0]["data"]["delta"], "世界");
        assert_eq!(p.text, "你好，世界");
        assert_eq!(p.offset, 5);
        // A finalized later block continues from the rewritten tail.
        let next = json!({"cursor":4,"change_type":"text_block","turn_id":"fixture-child","item_id":"fixture-child:text-1","payload":{"content":"！","content_offset":5,"field":"content"}});
        let events = p.apply(&next).unwrap();
        assert_eq!(events[0]["data"]["delta"], "！");
        assert_eq!(p.text, "你好，世界！");
        // A rewrite that would alter already-applied text is rejected.
        let corrupted = json!({"cursor":5,"change_type":"text_block","turn_id":"fixture-child","item_id":"fixture-child:text-1","payload":{"content":"完全不同的内容","content_offset":0,"field":"content"}});
        assert!(p.apply(&corrupted).is_err());
        // A genuine offset gap stays an error.
        let gap = json!({"cursor":6,"change_type":"text_block","turn_id":"fixture-child","item_id":"fixture-child:text-1","payload":{"content":"跳","content_offset":9,"field":"content"}});
        assert!(p.apply(&gap).is_err());
    }

    #[test]
    fn utf16_byte_index_maps_boundaries_and_rejects_surrogate_splits() {
        assert_eq!(utf16_byte_index("你好", 0), Some(0));
        assert_eq!(utf16_byte_index("你好", 1), Some(3));
        assert_eq!(utf16_byte_index("你好", 2), Some(6));
        // Out-of-range and mid-character indexes have no byte position.
        assert_eq!(utf16_byte_index("你好", 5), None);
        // A surrogate pair occupies two UTF-16 units; splitting it is invalid.
        assert_eq!(utf16_byte_index("🐣x", 1), None);
        assert_eq!(utf16_byte_index("🐣x", 2), Some(4));
        assert_eq!(utf16_byte_index("abc", 3), Some(3));
    }

    #[test]
    fn snapshot_recovers_partial_child_and_resumes_from_atomic_cursor() {
        let mut p = Projection {
            root: "fixture-root".into(),
            ..Default::default()
        };
        let events = p.restore(&json!({"cursor":40,
            "turns":[{"turn_id":"fixture-root","root_turn_id":"fixture-root","status":"completed"},
                {"turn_id":"fixture-child","root_turn_id":"fixture-root","status":"running"}],
            "items":[{"turn_id":"fixture-child","item_id":"fixture-child:text-1","kind":"assistant_message","revision":2,"payload":{"content":"","model_round":1}}],
            "blocks":[{"item_id":"fixture-child:text-1","data":{"content":"Recovered partial","field":"content","content_offset":0}}]})).unwrap();
        assert_eq!(p.cursor, 40);
        assert_eq!(p.text, "Recovered partial");
        assert!(events
            .iter()
            .any(|event| event["data"]["delta"] == "Recovered partial"));
        assert!(!p.settled(false));
        p.apply(&json!({"cursor":41,"change_type":"turn_upsert","turn_id":"fixture-child","payload":{"root_turn_id":"fixture-root","status":"cancelled"}})).unwrap();
        assert!(p.settled(true));
    }

    #[tokio::test]
    async fn background_commits_stream_before_completion_and_drain_terminal_output() {
        use wunder_server::{
            config::Config, config_store::ConfigStore, state::AppStateInitOptions,
        };
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
        let config_store = ConfigStore::new(temp.path().join("fixture.yaml"));
        config_store
            .update(|value| *value = config.clone())
            .await
            .unwrap();
        let state = Arc::new(
            AppState::new_with_options(
                config_store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        let root = state
            .storage
            .accept_thread_turn(
                "owner",
                "fixture-session",
                &json!({"content":"Fixture objective"}),
            )
            .unwrap();
        let root_id = root["turn_id"].as_str().unwrap().to_string();
        state
            .storage
            .update_thread_turn(
                "owner",
                "fixture-session",
                &root_id,
                "completed",
                "",
                &json!({}),
            )
            .unwrap();
        let mut goal = wunder_server::storage::SessionGoalRecord {
            goal_id: "fixture-goal".into(),
            session_id: "fixture-session".into(),
            user_id: "owner".into(),
            revision: 1,
            objective: "Fixture objective".into(),
            phase: "active".into(),
            blocked_code: None,
            blocked_message: None,
            max_goal_rounds: 256,
            rounds_started: 0,
            created_at: 0.0,
            updated_at: 0.0,
        };
        state.storage.upsert_session_goal(&goal).unwrap();
        let cursor = state
            .storage
            .latest_thread_change_seq_by_session("fixture-session")
            .unwrap();
        let (output, mut receiver) = mpsc::channel(128);
        let cancel = CancellationToken::new();
        let task_state = state.clone();
        let task_root = root_id.clone();
        let task_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            watch(
                &task_state,
                "owner",
                "fixture-session",
                &task_root,
                cursor,
                &output,
                &task_cancel,
                true,
            )
            .await
        });
        // A completed root must remain subscribed throughout the scheduling gap.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!task.is_finished());
        let child = state
            .storage
            .accept_thread_turn(
                "owner",
                "fixture-session",
                &json!({"content":"Internal", "root_user_round":1}),
            )
            .unwrap();
        let child_id = child["turn_id"].as_str().unwrap();
        let item = format!("{child_id}:text-1");
        state.storage.append_thread_item("owner", &json!({"session_id":"fixture-session","turn_id":child_id,
            "item_id":item,"kind":"assistant_message","role":"assistant","model_round":1,"content":"","status":"running"})).unwrap();
        state
            .storage
            .upsert_thread_text_block(
                "owner",
                "fixture-session",
                &json!({"item_id":item,"field":"content","block_index":0,"event_id":1,
            "data":{"field":"content","content_offset":0,"content":"Fixture partial"}}),
            )
            .unwrap();
        let partial = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(NativeChatEvent::Event(event)) = receiver.recv().await {
                    if event["event"] == "llm_output_delta" {
                        break event;
                    }
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(partial["data"]["delta"], "Fixture partial");
        assert!(!task.is_finished());
        goal.phase = "complete".into();
        state.storage.upsert_session_goal(&goal).unwrap();
        // The goal is complete before the execution commits its final answer.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!task.is_finished());
        state.storage.append_thread_item("owner", &json!({"session_id":"fixture-session","turn_id":child_id,
            "item_id":item,"kind":"assistant_message","role":"assistant","model_round":1,"content":"Fixture final","status":"completed"})).unwrap();
        state
            .storage
            .update_thread_turn(
                "owner",
                "fixture-session",
                child_id,
                "completed",
                "",
                &json!({}),
            )
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let mut events = Vec::new();
        while let Some(NativeChatEvent::Event(event)) = receiver.recv().await {
            events.push(event);
        }
        let answer = events
            .iter()
            .position(|event| event["data"]["content"] == "Fixture final")
            .unwrap();
        let terminal = events
            .iter()
            .position(|event| event["event"] == "turn_terminal")
            .unwrap();
        assert!(answer < terminal);
        // Closing the UI subscriber stops observation, not the goal or task.
        goal.phase = "active".into();
        state.storage.upsert_session_goal(&goal).unwrap();
        let (output, receiver) = mpsc::channel(1);
        drop(receiver);
        tokio::time::timeout(
            Duration::from_secs(1),
            watch(
                &state,
                "owner",
                "fixture-session",
                &root_id,
                state
                    .storage
                    .latest_thread_change_seq_by_session("fixture-session")
                    .unwrap(),
                &output,
                &cancel,
                true,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            state
                .storage
                .get_session_goal("owner", "fixture-session")
                .unwrap()
                .unwrap()
                .status,
            "active"
        );
        // Explicit stop must clear the goal even when UI delivery is full.
        state
            .user_store
            .upsert_chat_session(&wunder_server::storage::ChatSessionRecord {
                session_id: "fixture-session".into(),
                user_id: "owner".into(),
                title: "Fixture".into(),
                status: "active".into(),
                created_at: 0.0,
                updated_at: 0.0,
                last_message_at: 0.0,
                agent_id: None,
                workspace_id: None,
                tool_overrides: vec![],
                parent_session_id: None,
                parent_message_id: None,
                spawn_label: None,
                spawned_by: None,
            })
            .unwrap();
        let (output, _receiver) = mpsc::channel(1);
        output.try_send(NativeChatEvent::Queued).unwrap();
        cancel.cancel();
        tokio::time::timeout(
            Duration::from_secs(3),
            watch(
                &state,
                "owner",
                "fixture-session",
                &root_id,
                state
                    .storage
                    .latest_thread_change_seq_by_session("fixture-session")
                    .unwrap(),
                &output,
                &cancel,
                true,
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(state
            .storage
            .get_session_goal("owner", "fixture-session")
            .unwrap()
            .is_none());
    }
}
