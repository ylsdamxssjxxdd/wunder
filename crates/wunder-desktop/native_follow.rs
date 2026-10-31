//! Subscribe to background executions by durable cursor, scoped to one user turn.
use super::{cancel_chat, NativeChatEvent};
use anyhow::{anyhow, ensure, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wunder_server::{blocking, state::AppState, watch_thread_changes, ThreadChangeFrame};

#[derive(Default)]
struct Projection {
    root: String,
    execution: String,
    admitted: String,
    status: String,
    cursor: i64,
    // Only the active execution is retained; late records from prior executions
    // cannot reset its model-round clock or overwrite its answer.
    items: HashMap<String, (i64, String, i64)>,
    text_item: String,
    text: String,
    reasoning: String,
    // A question panel is stored on the assistant message metadata rather
    // than as a standalone durable item. Keep its item identity so a replay
    // can publish it once when the desktop resumes a background turn.
    question_panel_item: String,
    user_round: i64,
}

impl Projection {
    #[cfg(test)]
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
        self.user_round = turns
            .iter()
            .find(|turn| turn["turn_id"] == self.root)
            .and_then(|turn| turn["user_turn_index"].as_i64())
            .unwrap_or(self.user_round);
        let execution = latest["turn_id"]
            .as_str()
            .ok_or_else(|| anyhow!("snapshot execution missing"))?;
        self.execution.clear();
        self.items.clear();
        self.text.clear();
        self.text_item.clear();
        self.reasoning.clear();
        self.question_panel_item.clear();
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
        self.status = latest["status"].as_str().unwrap_or_default().into();
        self.cursor = cursor;
        events.push(json!({"event":"native_execution_status","data":{"root_turn_id":self.root,"status":self.status,"user_round":self.user_round}}));
        Ok(events)
    }

    fn apply(&mut self, data: &Value) -> Result<Vec<Value>> {
        let seq = data["cursor"].as_i64().unwrap_or(0);
        if seq <= self.cursor {
            return Ok(vec![]);
        }
        // Canonical rollout positions include non-display records (for
        // example model context), so cursor values are strictly monotonic but
        // intentionally sparse. A gap is not a missing UI event.
        ensure!(seq > self.cursor, "native durable cursor did not advance");
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
                if let Some(round) = p["user_turn_index"].as_i64().filter(|round| *round > 0) {
                    self.user_round = round;
                }
                if p["status"] == "accepted" {
                    // Admission is a separate durable revision. Remember its
                    // identity without exposing it as a running execution.
                    if self.execution != execution {
                        self.admitted = execution.into();
                    }
                    return Ok(events);
                }
                if self.execution != execution {
                    // Acceptance starts an execution. Status changes for old
                    // executions must never switch the active execution back.
                    if !matches!(p["status"].as_str(), Some("queued" | "running"))
                        || (!self.execution.is_empty()
                            && self.admitted != execution
                            && data["revision"].as_i64().unwrap_or(1) != 1)
                    {
                        return Ok(events);
                    }
                    self.execution = execution.into();
                    self.admitted.clear();
                    self.items.clear();
                    self.text_item.clear();
                    self.text.clear();
                    self.reasoning.clear();
                    self.question_panel_item.clear();
                    events.push(json!({"event":"native_execution_started","data":{"root_turn_id":self.root,"turn_id":execution,"user_round":self.user_round}}));
                }
                self.status = p["status"].as_str().unwrap_or_default().into();
                events
                    .push(json!({"event":"native_execution_status","data":{"root_turn_id":self.root,"status":self.status,"user_round":self.user_round}}));
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
                // The durable cursor rejects replay globally. Item revisions
                // are a bounded hot cache, never a maximum task length.
                if !self.items.contains_key(id) && self.items.len() >= 2048 {
                    self.items.retain(|key, _| key == &self.text_item);
                }
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
                        self.reasoning.clear();
                    }
                    if let Some(reasoning) = payload["reasoning"].as_str() {
                        // Completion snapshots repeat the streamed prefix. Only
                        // send its missing suffix, before the associated body.
                        ensure!(
                            reasoning.len() <= 8 * 1024 * 1024,
                            "native reasoning limit exceeded"
                        );
                        if let Some(delta) = reasoning
                            .strip_prefix(&self.reasoning)
                            .filter(|s| !s.is_empty())
                        {
                            events.push(json!({"event":"reasoning_delta","data":{"delta":delta,"model_round":round}}));
                            self.reasoning = reasoning.into();
                        }
                    }
                    if let Some(content) =
                        payload["content"].as_str().filter(|text| !text.is_empty())
                    {
                        ensure!(
                            content.len() <= 8 * 1024 * 1024,
                            "native text limit exceeded"
                        );
                        self.text = content.into();
                        events.push(json!({"event":"llm_output","data":{"content":content,"model_round":round}}));
                    }
                    if let Some(stats) = native_stats_payload(payload) {
                        events.push(json!({"event":"native_stats","data":stats}));
                    }
                    // `question_panel` is emitted live by the orchestrator,
                    // but it is not a standalone thread-log item. A native
                    // subscriber that starts after the live event (for
                    // example after navigation or a queued hand-off) must
                    // reconstruct it from the persisted assistant metadata.
                    if let Some(panel) = pending_question_panel(payload) {
                        if self.question_panel_item != id {
                            self.question_panel_item = id.to_string();
                            // The composer answers against the durable waiting
                            // turn row and keys replay suppression on the same
                            // identity; without it every identical question
                            // collapses into one consumed panel.
                            let mut panel = panel;
                            panel["turn_id"] = json!(execution);
                            events.push(json!({"event":"question_panel","data":panel}));
                        }
                    } else if self.question_panel_item == id
                        && payload.pointer("/meta/panel").is_some()
                    {
                        self.question_panel_item.clear();
                        events.push(json!({"event":"question_panel_clear","data":{}}));
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
                } else if kind == "question_panel" {
                    // A background round (goal continuation) publishes its
                    // panel as its own durable item, not on assistant meta.
                    // Without this the follower never told the composer that
                    // the thread had stopped waiting for the reader.
                    if let Some(panel) = durable_question_panel(payload) {
                        let id = format!("panel:{id}");
                        if self.question_panel_item != id {
                            self.question_panel_item = id;
                            let mut panel = panel;
                            panel["turn_id"] = json!(execution);
                            events.push(json!({"event":"question_panel","data":panel}));
                        }
                    }
                } else if kind == "compaction" {
                    let mut detail = payload.clone();
                    detail["status"] = p["status"].clone();
                    detail["item_id"] = json!(id);
                    events.push(json!({"event":"compaction","data":detail}));
                } else if kind == "subagent_run" {
                    // One child run, one stable item: every progress revision
                    // rewrites the same card, so the UI can patch it in place.
                    let mut card = payload.clone();
                    card["item_id"] = json!(id);
                    events.push(json!({"event":"subagent_run","data":card}));
                } else if kind == "queue" && p["status"] == "queued" {
                    events.push(json!({"event":"queue_update","data":payload}));
                }
            }
            _ => {}
        }
        Ok(events)
    }

    fn control_status(
        &mut self,
        row: &wunder_server::stable_core::thread_view::ChatViewRow,
    ) -> Vec<Value> {
        if row.root_turn_id != self.root {
            return vec![];
        }
        self.user_round = row.user_round;
        if self.execution.is_empty() {
            self.execution = row.execution_turn_id.clone();
        } else if self.execution != row.execution_turn_id {
            self.admitted = row.execution_turn_id.clone();
        }
        if self.status == row.status {
            return vec![];
        }
        self.status = row.status.clone();
        vec![
            json!({"event":"native_execution_status","data":{"root_turn_id":self.root,
            "status":self.status,"user_round":self.user_round}}),
        ]
    }

    fn settled(&self, goal_active: bool) -> bool {
        matches!(
            self.status.as_str(),
            "failed"
                | "cancelled"
                | "interrupted"
                | "aborted"
                | "rejected"
                | "waiting_input"
                | "waiting_user_input"
        ) || (self.status == "completed" && !goal_active)
    }
}

/// Keep background replay statistics independent from the assistant body.
/// Canonical assistant items may store them under `meta.message_stats`, under
/// `stats`, or directly beside `content`/`reasoning`; only the small numeric
/// usage/timing fields are forwarded to the UI.
fn native_stats_payload(payload: &Value) -> Option<Value> {
    if let Some(stats) = payload
        .pointer("/meta/message_stats")
        .or_else(|| payload.get("stats"))
    {
        return Some(stats.clone());
    }
    let map = payload.as_object()?;
    let keys = [
        "interaction_duration_s",
        "prefill_duration_s",
        "prefill_duration_total_s",
        "decode_duration_s",
        "decode_duration_total_s",
        "decode_output_tokens",
        "decode_tokens",
        "decode_speed_tps",
        "prefill_speed_tps",
        "visible_decode_tokens",
        "visible_decode_duration_s",
        "visible_decode_speed_tps",
        "visible_decode_measured",
        "avg_model_round_speed_tps",
        "avg_model_round_decode_speed_tps",
        "avg_model_round_speed_rounds",
        "context_occupancy_tokens",
        "contextTokens",
        "request_consumed_tokens",
        "consumed_tokens",
        "toolCalls",
        "tool_calls",
        "model_request_count",
        "account_credits_consumed",
        "usage",
        "round_usage",
        "stream_timing",
    ];
    let mut stats = serde_json::Map::new();
    for key in keys {
        if let Some(value) = map.get(key).filter(|value| !value.is_null()) {
            stats.insert(key.to_owned(), value.clone());
        }
    }
    (!stats.is_empty()).then_some(Value::Object(stats))
}

/// Extract only a pending panel from an assistant item. The canonical item
/// stores the panel at `payload.meta.panel`; the second path keeps recovery
/// tolerant of the older double-wrapped preview shape.
fn pending_question_panel(payload: &Value) -> Option<Value> {
    let panel = payload
        .pointer("/meta/panel")
        .or_else(|| payload.pointer("/payload/meta/panel"))?;
    if !panel.is_object()
        || panel["questions"]
            .as_array()
            .is_none_or(|questions| questions.is_empty())
    {
        return None;
    }
    if matches!(panel["status"].as_str(), Some("answered" | "dismissed")) {
        return None;
    }
    Some(panel.clone())
}

/// A standalone `question_panel` thread-log item. Its envelope status only says
/// the item was persisted; the panel itself is still pending until the reader
/// answers it (or the waiting turn settles), so the synthetic status is always
/// `pending`.
fn durable_question_panel(payload: &Value) -> Option<Value> {
    let questions = payload.get("questions")?.as_array()?;
    if questions.is_empty() {
        return None;
    }
    Some(json!({
        "questions": questions,
        "keep_open": payload.get("keep_open").and_then(Value::as_bool).unwrap_or(true),
        "status": "pending",
    }))
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
    // Subscribe before the initial durable goal read so a concurrent pause or
    // clear cannot be lost between initialization and the first history frame.
    let mut goal_events = state.kernel.orchestrator.subscribe_goal_updates(session);
    let mut control_events = state.kernel.orchestrator.subscribe_thread_updates(session);
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
    // Admission can precede the supplied cursor (queued/background starts).
    // Resolve the root ordinal once, without replaying the full conversation.
    let storage = state.storage.clone();
    let (owner, target, root_id) = (user.to_string(), session.to_string(), root.to_string());
    let (user_round, mut goal_active) = blocking::run_db("native.follow.identity", move || {
        let user_round = storage
            .get_thread_turn(&owner, &target, &root_id, -1, 1, false)?
            .and_then(|turn| turn["user_turn_index"].as_i64())
            .unwrap_or_default();
        let goal_active = storage
            .get_session_goal(&owner, &target)?
            .is_some_and(|goal| goal.phase == "active");
        Ok((user_round, goal_active))
    })
    .await?;
    projection.user_round = user_round;
    tokio::select! {
        _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(()); }
        result = output.send(NativeChatEvent::Event(json!({"event":"native_turn_identity","data":{"root_turn_id":root,"user_round":projection.user_round}}))) => {
            if result.is_err() { return Ok(()); }
        }
    }
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
                json!({"event":"native_execution_status","data":{"root_turn_id":root,"status":"continuing","user_round":projection.user_round}}),
            )) => { if result.is_err() { return Ok(()); } }
        }
    }
    let mut refresh_control = true;
    loop {
        if refresh_control {
            if !sync_control(state, user, session, &mut projection, output, cancel).await? {
                return Ok(());
            }
            refresh_control = false;
            if projection.settled(goal_active) {
                let terminal = json!({"event":"turn_terminal","data":{
                    "status":projection.status,"user_round":projection.user_round}});
                tokio::select! {
                    _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; },
                    _ = output.closed() => {},
                    _ = output.send(NativeChatEvent::Event(terminal)) => {},
                }
                return Ok(());
            }
        }
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
                        if !restore_target_turn(state,user,session,&mut projection,output,cancel).await? {
                            return Ok(());
                        }
                        feed = watch_thread_changes(state.clone(), session.into(), projection.cursor, Some(feed_cancel.clone())).await?;
                    }
                    ThreadChangeFrame::Overflow { .. } => {
                        feed = watch_thread_changes(state.clone(), session.into(), projection.cursor, Some(feed_cancel.clone())).await?;
                    }
                }
            }
            changed = control_events.changed() => {
                if changed.is_err() { return Ok(()); }
                control_events.borrow_and_update();
                refresh_control = true;
                continue;
            }
            changed = goal_events.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                // Publishers send a complete post-commit projection (or null
                // after a clear), so goal changes need no second SQLite read.
                goal_active = goal_events.borrow_and_update()["phase"] == "active";
            }
        }
        // Canonical terminal events follow all completed items for this turn.
        // The feed is ordered; model-context positions are intentionally not
        // UI frames and must not force an idle watermark polling loop.
        if projection.settled(goal_active) {
            let status = projection.status.clone();
            tokio::select! {
                _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; },
                _ = output.closed() => {},
                _ = output.send(NativeChatEvent::Event(json!({"event":"turn_terminal","data":{"status":status,"user_round":projection.user_round}}))) => {},
            }
            return Ok(());
        }
    }
}

/// Recovery materializes one item page at a time. A generic snapshot can be
/// truncated by byte/item limits and can omit continuation execution turns;
/// targeting the user root keeps its full completed output recoverable.
async fn restore_target_turn(
    state: &Arc<AppState>,
    user: &str,
    session: &str,
    projection: &mut Projection,
    output: &mpsc::Sender<NativeChatEvent>,
    cancel: &CancellationToken,
) -> Result<bool> {
    let storage = state.storage.clone();
    let (owner, target, round) = (user.to_owned(), session.to_owned(), projection.user_round);
    let page = tokio::select! {
        _=cancel.cancelled()=>{cancel_chat(state,user,session).await?;return Ok(false);},
        _=output.closed()=>return Ok(false),
        page=blocking::run_db("native.follow.restore_head",move||storage.thread_view_window(
            &owner,&target,"chat",Some(round.saturating_add(1)),None,None,1))=>page?,
    };
    let row = page
        .chat
        .into_iter()
        .find(|row| row.root_turn_id == projection.root)
        .ok_or_else(|| anyhow!("native root no longer exists"))?;
    let watermark = page.view_seq;
    let root = projection.root.clone();
    *projection = Projection {
        root: root.clone(),
        user_round: row.user_round,
        ..Default::default()
    };
    let initial = json!({"cursor":1,"change_type":"turn_upsert","turn_id":row.execution_turn_id,
        "payload":{"root_turn_id":root,"status":"running","user_turn_index":row.user_round}});
    for event in projection.apply(&initial)? {
        tokio::select! {
            _=cancel.cancelled()=>{cancel_chat(state,user,session).await?;return Ok(false);},
            result=output.send(NativeChatEvent::Event(event))=>if result.is_err(){return Ok(false);},
        }
    }
    let mut after = -1;
    loop {
        let storage = state.storage.clone();
        let (owner, target, root) = (user.to_owned(), session.to_owned(), root.clone());
        let page = tokio::select! {
            _=cancel.cancelled()=>{cancel_chat(state,user,session).await?;return Ok(false);},
            _=output.closed()=>return Ok(false),
            page=blocking::run_db("native.follow.restore_items",move||storage.get_thread_turn(
                &owner,&target,&root,after,100,false))=>page?,
        }
        .ok_or_else(|| anyhow!("native root disappeared during recovery"))?;
        for item in page["items"].as_array().into_iter().flatten() {
            let cursor = item["created_seq"].as_i64().unwrap_or(0);
            if item["turn_id"] != row.execution_turn_id || cursor > watermark {
                continue;
            }
            let change = json!({"cursor":cursor,"change_type":"item_upsert",
                "turn_id":row.execution_turn_id,"item_id":item["item_id"],"payload":item});
            for event in projection.apply(&change)? {
                tokio::select! {
                    _=cancel.cancelled()=>{cancel_chat(state,user,session).await?;return Ok(false);},
                    result=output.send(NativeChatEvent::Event(event))=>if result.is_err(){return Ok(false);},
                }
            }
        }
        if page["has_more"] != true {
            break;
        }
        let next = page["next_after"]
            .as_i64()
            .ok_or_else(|| anyhow!("native recovery item cursor missing"))?;
        ensure!(next > after, "native recovery item cursor did not advance");
        after = next;
    }
    projection.cursor = watermark;
    for event in projection.control_status(&row) {
        tokio::select! {
            _=cancel.cancelled()=>{cancel_chat(state,user,session).await?;return Ok(false);},
            result=output.send(NativeChatEvent::Event(event))=>if result.is_err(){return Ok(false);},
        }
    }
    Ok(true)
}

/// Reconcile control-only edges after draining every completed item committed
/// before the captured watermark. This keeps waiting/cancellation event-driven
/// without letting a status notification overtake the final history item.
async fn sync_control(
    state: &Arc<AppState>,
    user: &str,
    session: &str,
    projection: &mut Projection,
    output: &mpsc::Sender<NativeChatEvent>,
    cancel: &CancellationToken,
) -> Result<bool> {
    let storage = state.storage.clone();
    let (owner, target, round) = (user.to_owned(), session.to_owned(), projection.user_round);
    let page = tokio::select! {
        _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(false); },
        _ = output.closed() => return Ok(false),
        page = blocking::run_db("native.follow.control", move || {
            storage.thread_view_window(&owner, &target, "chat",
                Some(round.saturating_add(1)), None, None, 1)
        }) => page?,
    };
    let watermark = page.view_seq;
    while projection.cursor < watermark {
        let storage = state.storage.clone();
        let target = session.to_owned();
        let after = projection.cursor;
        let changes = tokio::select! {
            _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(false); },
            _ = output.closed() => return Ok(false),
            changes = blocking::run_db("native.follow.control_history", move ||
                storage.list_thread_changes_by_session(&target, after, 200)) => changes?,
        };
        let mut progressed = false;
        for mut change in changes {
            if change["change_type"] == "snapshot_required" {
                // The feed owns rollout replacement/snapshot recovery. Avoid
                // applying a control state against a different history epoch.
                return Ok(true);
            }
            let cursor = change["change_seq"].as_i64().unwrap_or(0);
            if cursor > watermark {
                break;
            }
            change["cursor"] = json!(cursor);
            for event in projection.apply(&change)? {
                tokio::select! {
                    _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(false); },
                    result = output.send(NativeChatEvent::Event(event)) => {
                        if result.is_err() { return Ok(false); }
                    }
                }
            }
            progressed |= projection.cursor > after;
        }
        if !progressed {
            // Remaining canonical ordinals can be model-only context records.
            projection.cursor = watermark;
            break;
        }
    }
    if let Some(row) = page
        .chat
        .iter()
        .find(|row| row.root_turn_id == projection.root)
    {
        for event in projection.control_status(row) {
            tokio::select! {
                _ = cancel.cancelled() => { cancel_chat(state, user, session).await?; return Ok(false); },
                result = output.send(NativeChatEvent::Event(event)) => {
                    if result.is_err() { return Ok(false); }
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn canonical_cursor_gaps_are_accepted_and_replays_are_ignored() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            cursor: 3,
            ..Default::default()
        };
        let event = json!({"cursor":8,"change_type":"turn_upsert","turn_id":"fixture-root","payload":{"status":"running"}});
        assert!(!projection.apply(&event).unwrap().is_empty());
        assert_eq!(projection.cursor, 8);
        assert!(projection.apply(&event).unwrap().is_empty());
        let stale = json!({"cursor":5,"change_type":"turn_upsert","turn_id":"fixture-root","payload":{"status":"completed"}});
        assert!(projection.apply(&stale).unwrap().is_empty());
        assert_eq!(projection.status, "running");
    }
    #[test]
    fn accepted_continuation_starts_only_after_running_revision() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            execution: "fixture-root".into(),
            status: "completed".into(),
            ..Default::default()
        };
        assert!(projection.apply(&json!({"cursor":1,"revision":1,"change_type":"turn_upsert","turn_id":"fixture-child","payload":{"root_turn_id":"fixture-root","status":"accepted","user_turn_index":57}})).unwrap().is_empty());
        assert_eq!(projection.execution, "fixture-root");
        let events = projection.apply(&json!({"cursor":2,"revision":2,"change_type":"turn_status","turn_id":"fixture-child","payload":{"root_turn_id":"fixture-root","status":"running"}})).unwrap();
        assert_eq!(projection.execution, "fixture-child");
        assert_eq!(events[0]["event"], "native_execution_started");
        assert_eq!(events[0]["data"]["user_round"], 57);
    }

    #[test]
    fn long_follow_rolls_revision_cache_without_stopping_the_execution() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            execution: "fixture-root".into(),
            ..Default::default()
        };
        for index in 0..2200 {
            projection.apply(&json!({"cursor":index+1,"change_type":"item_upsert","turn_id":"fixture-root","payload":{"item_id":format!("fixture-tool-{index}"),"revision":1,"kind":"tool_call","status":"completed","payload":{"tool":"read_file"}}})).unwrap();
        }
        assert!(projection.items.len() <= 2048);
        assert_eq!(projection.cursor, 2200);
        assert_eq!(projection.execution, "fixture-root");
    }
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
    fn completed_items_duplicate_replay_and_foreign_turn_are_isolated() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            execution: "fixture-child".into(),
            ..Default::default()
        };
        let item = json!({"cursor":7,"change_type":"item_upsert","turn_id":"fixture-child",
            "payload":{"item_id":"fixture-child:text-1","kind":"assistant_message","revision":1,
                "payload":{"content":"Complete reply 🐣","reasoning":"Complete thought","model_round":1}}});
        let events = projection.apply(&item).unwrap();
        assert_eq!(events[0]["event"], "reasoning_delta");
        assert_eq!(events[0]["data"]["delta"], "Complete thought");
        assert_eq!(events[1]["event"], "llm_output");
        assert_eq!(events[1]["data"]["content"], "Complete reply 🐣");
        assert!(projection.apply(&item).unwrap().is_empty());
        let mut duplicate = item;
        duplicate["cursor"] = json!(9);
        assert!(projection.apply(&duplicate).unwrap().is_empty());
        projection
            .apply(
                &json!({"cursor":10,"change_type":"turn_upsert","turn_id":"other",
            "payload":{"status":"queued"}}),
            )
            .unwrap();
        assert_eq!(projection.execution, "fixture-child");
        assert_eq!(projection.text, "Complete reply 🐣");
        assert_eq!(projection.reasoning, "Complete thought");
    }

    #[test]
    fn pending_question_panel_is_replayed_from_assistant_metadata() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            execution: "fixture-root".into(),
            ..Default::default()
        };
        let item = json!({
            "cursor": 4,
            "change_type": "item_upsert",
            "turn_id": "fixture-root",
            "payload": {
                "item_id": "fixture-root:text-1",
                "kind": "assistant_message",
                "revision": 1,
                "payload": {
                    "model_round": 1,
                    "content": "等待选择",
                    "meta": {
                        "type": "question_panel",
                        "panel": {
                            "status": "pending",
                            "keep_open": true,
                            "questions": [{
                                "question": "继续吗？",
                                "options": [{"label": "继续"}]
                            }]
                        }
                    }
                }
            }
        });
        let events = projection.apply(&item).expect("panel replay");
        let panel = events
            .iter()
            .find(|event| event["event"] == "question_panel")
            .expect("question panel event");
        assert_eq!(panel["data"]["status"], "pending");
        assert_eq!(panel["data"]["questions"][0]["question"], "继续吗？");
        // The composer answers and de-duplicates against the waiting turn row.
        assert_eq!(panel["data"]["turn_id"], "fixture-root");
        // A duplicate revision is ignored and must not reopen the card.
        assert!(projection.apply(&item).unwrap().is_empty());
    }

    #[test]
    fn standalone_question_panel_item_reaches_the_composer() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            execution: "fixture-round".into(),
            ..Default::default()
        };
        let item = json!({
            "cursor": 4,
            "change_type": "item_upsert",
            "turn_id": "fixture-round",
            "payload": {
                "item_id": "fixture-round:question-panel-2",
                "kind": "question_panel",
                "status": "completed",
                "revision": 1,
                "payload": {
                    "kind": "question_panel",
                    "status": "completed",
                    "keep_open": true,
                    "questions": [{
                        "question": "继续吗？",
                        "options": [{"label": "继续"}, {"label": "暂停"}]
                    }]
                }
            }
        });
        let events = projection.apply(&item).expect("panel item");
        let panel = events
            .iter()
            .find(|event| event["event"] == "question_panel")
            .expect("question panel event");
        assert_eq!(panel["data"]["status"], "pending");
        assert_eq!(panel["data"]["questions"][0]["question"], "继续吗？");
        // The answer must settle the waiting goal round, not the root command.
        assert_eq!(panel["data"]["turn_id"], "fixture-round");
        assert!(projection.apply(&item).unwrap().is_empty());
    }

    #[test]
    fn snapshot_restores_full_completed_content_and_reasoning() {
        let mut projection = Projection {
            root: "fixture-root".into(),
            ..Default::default()
        };
        let events = projection.restore(&json!({"cursor":20,
            "turns":[{"turn_id":"fixture-child","root_turn_id":"fixture-root","status":"completed"}],
            "items":[{"turn_id":"fixture-child","item_id":"fixture-child:text-1",
                "kind":"assistant_message","revision":1,"payload":{"model_round":1,
                    "reasoning":"Recovered thought 🐣","content":"Recovered reply"}}]})).unwrap();
        assert_eq!(projection.reasoning, "Recovered thought 🐣");
        assert_eq!(projection.text, "Recovered reply");
        assert_eq!(projection.cursor, 20);
        assert!(events
            .iter()
            .any(|event| event["event"] == "reasoning_delta"
                && event["data"]["delta"] == "Recovered thought 🐣"));
        assert!(events
            .iter()
            .any(|event| event["event"] == "llm_output"
                && event["data"]["content"] == "Recovered reply"));
        assert!(projection.settled(false));
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
            "items":[{"turn_id":"fixture-child","item_id":"fixture-child:text-1","kind":"assistant_message","revision":1,"payload":{"content":"Recovered partial","model_round":1}}]})).unwrap();
        assert_eq!(p.cursor, 40);
        assert_eq!(p.text, "Recovered partial");
        assert!(events
            .iter()
            .any(|event| event["data"]["content"] == "Recovered partial"));
        assert!(!p.settled(false));
        p.apply(&json!({"cursor":41,"change_type":"turn_upsert","turn_id":"fixture-child","payload":{"root_turn_id":"fixture-root","status":"cancelled"}})).unwrap();
        assert!(p.settled(true));
    }

    #[tokio::test]
    async fn recovery_pages_the_target_root_and_restores_complete_continuation() {
        use wunder_server::{
            config::Config, config_store::ConfigStore, stable_core::thread_store::*,
            state::AppStateInitOptions,
        };
        let directory = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = directory.path().join("data").to_string_lossy().into_owned();
        config.workspace.root = directory
            .path()
            .join("workspace")
            .to_string_lossy()
            .into_owned();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(directory.path().join("config.yaml"));
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
        let thread = ThreadId::new("fixture-thread").unwrap();
        let accepted = state
            .storage
            .accept_thread_turn(
                "fixture-user",
                thread.as_str(),
                &json!({"content":"sample input"}),
            )
            .unwrap();
        let root = TurnId::new(accepted["turn_id"].as_str().unwrap()).unwrap();
        let rollout = RolloutId::new("fixture-rollout").unwrap();
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
                    source: "fixture".into(),
                    initial_system_prompt: "sample instruction".into(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: Value::Null,
                },
            })
            .await
            .unwrap();
        state
            .thread_store
            .begin_turn("fixture-user", &thread, &root, "fixture-model")
            .await
            .unwrap();
        let tools = |turn: &TurnId, prefix: &str, count: usize| {
            (0..count)
                .map(|index| {
                    CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                        item_id: ItemId::new(format!("{prefix}-{index}")).unwrap(),
                        turn_id: turn.clone(),
                        root_turn_id: Some(root.clone()),
                        created_at_ms: 3,
                        kind: CanonicalItemKind::ToolCall,
                        payload: json!({"tool":"read_file","result":"sample result"}),
                        display_output: None,
                        display_output_truncated: false,
                    }))
                })
                .collect::<Vec<_>>()
        };
        state
            .thread_store
            .append_canonical(&thread, tools(&root, "fixture-root-tool", 105))
            .await
            .unwrap();
        state
            .storage
            .update_thread_turn(
                "fixture-user",
                thread.as_str(),
                root.as_str(),
                "completed",
                "",
                &json!({}),
            )
            .unwrap();
        let accepted = state
            .storage
            .accept_thread_turn(
                "fixture-user",
                thread.as_str(),
                &json!({"content":"sample continuation","root_user_round":1}),
            )
            .unwrap();
        let child = TurnId::new(accepted["turn_id"].as_str().unwrap()).unwrap();
        state
            .thread_store
            .begin_turn("fixture-user", &thread, &child, "fixture-model")
            .await
            .unwrap();
        let mut records = tools(&child, "fixture-child-tool", 120);
        let complete_text = "Complete reply 🐣".repeat(1024);
        let complete_reasoning = "Complete thought 🐣".repeat(1024);
        records.push(CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
            item_id:ItemId::new(format!("{}:text-1",child.as_str())).unwrap(),turn_id:child.clone(),
            root_turn_id:Some(root.clone()),created_at_ms:4,kind:CanonicalItemKind::AssistantMessage,
            payload:json!({"content":complete_text,"reasoning":complete_reasoning,"model_round":1}),
            display_output:None,display_output_truncated:false,
        })));
        state
            .thread_store
            .append_canonical(&thread, records)
            .await
            .unwrap();
        state
            .storage
            .update_thread_turn(
                "fixture-user",
                thread.as_str(),
                child.as_str(),
                "completed",
                "",
                &json!({}),
            )
            .unwrap();
        let watermark = state
            .storage
            .latest_thread_change_seq_by_session(thread.as_str())
            .unwrap();
        // The tiny transport exercises page-by-page recovery under backpressure.
        let (output, mut receiver) = mpsc::channel(4);
        let collector = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(NativeChatEvent::Event(event)) = receiver.recv().await {
                events.push(event);
            }
            events
        });
        let mut projection = Projection {
            root: root.to_string(),
            user_round: 1,
            ..Default::default()
        };
        tokio::time::timeout(
            Duration::from_secs(5),
            restore_target_turn(
                &state,
                "fixture-user",
                thread.as_str(),
                &mut projection,
                &output,
                &CancellationToken::new(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        drop(output);
        let events = collector.await.unwrap();
        assert_eq!(projection.execution, child.as_str());
        assert_eq!(projection.text, complete_text);
        assert_eq!(projection.reasoning, complete_reasoning);
        assert_eq!(projection.cursor, watermark);
        assert!(projection.settled(false));
        assert_eq!(
            events
                .iter()
                .filter(|event| event["event"] == "tool_result")
                .count(),
            120
        );
        assert!(events
            .iter()
            .filter_map(|event| event["data"]["item_id"].as_str())
            .all(|id| !id.starts_with("fixture-root-tool")));
        let answer = events
            .iter()
            .position(|event| event["event"] == "llm_output")
            .unwrap();
        let terminal = events
            .iter()
            .position(|event| {
                event["event"] == "native_execution_status"
                    && event["data"]["status"] == "completed"
            })
            .unwrap();
        assert!(answer < terminal);
    }

    #[tokio::test]
    async fn control_only_queued_cancellation_wakes_without_rollout_or_idle_polling() {
        use wunder_server::{
            config::Config, config_store::ConfigStore, state::AppStateInitOptions,
        };
        let directory = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = directory.path().join("data").to_string_lossy().into_owned();
        config.workspace.root = directory
            .path()
            .join("workspace")
            .to_string_lossy()
            .into_owned();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(directory.path().join("config.yaml"));
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
        let turn = state
            .storage
            .accept_thread_turn(
                "fixture-user",
                "fixture-thread",
                &json!({"content":"sample input"}),
            )
            .unwrap();
        let root = turn["turn_id"].as_str().unwrap().to_owned();
        state
            .storage
            .update_thread_turn(
                "fixture-user",
                "fixture-thread",
                &root,
                "queued",
                "",
                &json!({"queued_input":{"queue_id":"fixture-queue","reason":"wait","priority":0}}),
            )
            .unwrap();
        let (output, mut receiver) = mpsc::channel(16);
        let task_state = state.clone();
        let task_root = root.clone();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            watch(
                &task_state,
                "fixture-user",
                "fixture-thread",
                &task_root,
                0,
                &output,
                &task_cancel,
                false,
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let Some(NativeChatEvent::Event(event)) = receiver.recv().await else {
                    panic!("follow closed")
                };
                if event["event"] == "native_execution_status"
                    && event["data"]["status"] == "queued"
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), receiver.recv())
                .await
                .is_err()
        );
        assert!(state
            .storage
            .update_thread_turn(
                "fixture-user",
                "fixture-thread",
                &root,
                "cancelled",
                "",
                &json!({"stop_reason":"fixture-stop"})
            )
            .unwrap());
        state
            .kernel
            .orchestrator
            .notify_thread_committed("fixture-thread", 0);
        let terminal = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let Some(NativeChatEvent::Event(event)) = receiver.recv().await else {
                    panic!("follow closed")
                };
                if event["event"] == "turn_terminal" {
                    break event;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(terminal["data"]["status"], "cancelled");
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            state
                .storage
                .latest_thread_change_seq_by_session("fixture-thread")
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn completed_history_is_drained_before_terminal_and_goal_updates_wake_settlement() {
        use wunder_server::{
            config::Config, config_store::ConfigStore, goal::GoalRef, stable_core::thread_store::*,
            state::AppStateInitOptions,
        };
        let directory = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.data_root = directory.path().join("data").to_string_lossy().into_owned();
        config.workspace.root = directory
            .path()
            .join("workspace")
            .to_string_lossy()
            .into_owned();
        config.skills.enabled.clear();
        let config_store = ConfigStore::new(directory.path().join("config.yaml"));
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
        let accepted = state
            .storage
            .accept_thread_turn(
                "owner",
                "fixture-thread",
                &json!({"content":"sample input"}),
            )
            .unwrap();
        let thread = ThreadId::new("fixture-thread").unwrap();
        let turn = TurnId::new(accepted["turn_id"].as_str().unwrap()).unwrap();
        let rollout = RolloutId::new("fixture-rollout").unwrap();
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
                    source: "fixture".into(),
                    initial_system_prompt: "sample instruction".into(),
                    initial_memory: None,
                    parent_thread_id: None,
                    history_base: None,
                    capabilities: Value::Null,
                },
            })
            .await
            .unwrap();
        state
            .thread_store
            .begin_turn("owner", &thread, &turn, "fixture-model")
            .await
            .unwrap();
        state
            .thread_store
            .append_canonical(
                &thread,
                vec![CanonicalRecord::ResponseItem(ResponseItem {
                    item_id: ItemId::new("fixture-context").unwrap(),
                    turn_id: turn.clone(),
                    root_turn_id: Some(turn.clone()),
                    created_at_ms: 3,
                    kind: CanonicalItemKind::AssistantMessage,
                    payload: json!({"content":"context"}),
                })],
            )
            .await
            .unwrap();
        state
            .storage
            .commit_thread_item(
                "owner",
                &json!({
                    "session_id":"fixture-thread", "turn_id":turn.as_str(),
                    "item_id":format!("{}:text-1", turn.as_str()),
                    "kind":"assistant_message", "status":"completed", "visibility":"user",
                    "content":"sample answer", "model_round":1,
                }),
            )
            .unwrap();
        state
            .storage
            .update_thread_turn(
                "owner",
                "fixture-thread",
                turn.as_str(),
                "completed",
                "",
                &json!({}),
            )
            .unwrap();
        let goal_service = state.kernel.orchestrator.goal_handle();
        let goal = goal_service
            .create(
                state.storage.clone(),
                "owner",
                thread.as_str(),
                "sample objective",
                Some(8),
            )
            .await
            .unwrap();
        let reference = GoalRef {
            goal_id: goal.record.goal_id,
            revision: goal.record.revision,
        };
        let (output, mut receiver) = mpsc::channel(32);
        let cancel = CancellationToken::new();
        let task_state = state.clone();
        let task_cancel = cancel.clone();
        let root_turn = turn.to_string();
        let task = tokio::spawn(async move {
            watch(
                &task_state,
                "owner",
                "fixture-thread",
                &root_turn,
                0,
                &output,
                &task_cancel,
                false,
            )
            .await
        });
        let mut events = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let Some(NativeChatEvent::Event(event)) = receiver.recv().await else {
                    panic!("follow closed");
                };
                let completed = event["event"] == "native_execution_status"
                    && event["data"]["status"] == "completed";
                events.push(event);
                if completed {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(
            !task.is_finished(),
            "active goal retains the completed follower"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(150), receiver.recv())
                .await
                .is_err(),
            "idle follow emits no timer probes"
        );
        goal_service
            .complete(state.storage.clone(), "owner", "fixture-thread", &reference)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        while let Some(NativeChatEvent::Event(event)) = receiver.recv().await {
            events.push(event);
        }
        let answer = events
            .iter()
            .position(|event| {
                event["event"] == "llm_output" && event["data"]["content"] == "sample answer"
            })
            .unwrap();
        let terminal = events
            .iter()
            .position(|event| event["event"] == "turn_terminal")
            .unwrap();
        assert!(answer < terminal);
    }
}
