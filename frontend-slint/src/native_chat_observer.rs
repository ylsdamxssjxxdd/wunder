//! Passive native updates are independent of locally submitted executions.
use super::*;
use wunder_desktop::native::{NativeChatTurn, NativeThreadUpdate, NativeThreadWatch};

#[derive(Default)]
pub(super) struct Observation {
    timer: Timer,
    session: String,
    watch: Option<NativeThreadWatch>,
    turns: Vec<NativeChatTurn>,
    /// Durable rows of the history, rebuilt only when a turn actually changes.
    rows: Vec<TimelineRow>,
    list_refreshed: Option<Instant>,
    ready: bool,
}

impl Observation {
    pub(super) fn detach(&mut self) {
        self.ready = false;
        self.watch = None;
        self.session.clear();
        self.turns.clear();
        self.rows.clear();
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }

    /// Remove a withdrawn queue placeholder before the watcher catches up.
    pub(super) fn remove_turn(&mut self, root_id: &str) -> bool {
        let before = self.turns.len();
        self.turns.retain(|turn| turn.root_id != root_id);
        if self.turns.len() == before {
            return false;
        }
        self.rows = project_history(&self.turns);
        true
    }

    /// Publish the durable history into the shared model without touching the
    /// live turn that is already appended to it.
    pub(super) fn publish(
        &self,
        app: &MainWindow,
        timeline: &Rc<RefCell<crate::timeline::Timeline>>,
    ) {
        let root = crate::native_chat::markdown_root(app);
        let mut live = timeline.borrow_mut();
        live.set_markdown_root(root);
        live.publish_history(self.rows.clone());
        live.hydrate_markdown_images();
        crate::native_chat::publish_timeline(app, &live);
    }
}

pub(super) fn install(app: &MainWindow, state: Rc<RefCell<State>>) {
    let weak = app.as_weak();
    let state_weak = Rc::downgrade(&state);
    state.borrow().observation.timer.start(
        TimerMode::Repeated,
        Duration::from_millis(33),
        move || {
            let (Some(app), Some(state)) = (weak.upgrade(), state_weak.upgrade()) else {
                return;
            };
            let mut current = state.borrow_mut();
            if current.active.is_some()
                || !current.parked.is_empty()
                || app.get_session_loading()
                || app.get_creating_session()
            {
                return;
            }
            let desktop = current.desktop.clone();
            // Cloned before the observation borrow: the history rows and a live
            // turn share one model.
            let timeline = current.timeline.clone();
            timeline
                .borrow_mut()
                .set_markdown_root(crate::native_chat::markdown_root(&app));
            let observation = &mut current.observation;
            let session = app.get_active_session_id().to_string();
            if observation.session != session {
                observation.detach();
                observation.session = session.clone();
                if !session.is_empty() {
                    observation.watch = Some(desktop.watch_chat(&session));
                }
            }
            let mut dirty = false;
            let mut reset = false;
            let started = Instant::now();
            for _ in 0..8 {
                let Some(update) = observation
                    .watch
                    .as_mut()
                    .and_then(NativeThreadWatch::try_recv)
                else {
                    break;
                };
                match update {
                    NativeThreadUpdate::Reset(turns) => {
                        observation.ready = true;
                        observation.turns = turns;
                        reset = true;
                        dirty = true;
                    }
                    NativeThreadUpdate::Turns(turns) => {
                        for turn in turns {
                            match observation
                                .turns
                                .iter_mut()
                                .find(|old| old.root_id == turn.root_id)
                            {
                                Some(old) => {
                                    if *old != turn {
                                        *old = turn;
                                        dirty = true;
                                    }
                                }
                                None => {
                                    observation.turns.push(turn);
                                    dirty = true;
                                }
                            }
                        }
                        if observation.turns.len() > 50 {
                            let drop = observation.turns.len() - 50;
                            observation.turns.drain(..drop);
                            reset = true;
                        }
                    }
                    NativeThreadUpdate::Context(usage) => apply_context_usage(&app, usage),
                    NativeThreadUpdate::Goal(goal) => {
                        app.set_goal_active(goal.active());
                        app.set_goal_objective(goal.objective.clone().into());
                        app.set_goal_paused(goal.paused());
                        let rows = app.get_conversations();
                        if let Some(index) = rows.iter().position(|row| row.id == session) {
                            if let Some(mut row) = rows.row_data(index) {
                                row.goal_active = goal.active();
                                row.goal_objective = goal.objective.clone().into();
                                row.goal_paused = goal.paused();
                                rows.set_row_data(index, row);
                                crate::navigation_ui::project(&app);
                            }
                        }
                    }
                    NativeThreadUpdate::Failed(error) => {
                        observation.watch = None;
                        app.set_status(error.into());
                    }
                }
                if started.elapsed() > Duration::from_millis(5) {
                    break;
                }
            }
            if dirty || reset {
                // The durable rows are rebuilt only when a turn changed, and
                // only for the turns the observer actually holds.
                observation.rows = project_history(&observation.turns);
                observation.publish(&app, &timeline);
                let busy = observation.turns.iter().any(|turn| {
                    matches!(
                        turn.assistant.stats_status.as_str(),
                        "正在生成…" | "正在排队"
                    ) || turn.assistant.stats_status.starts_with("正在排队 ·")
                });
                app.set_busy(busy);
                if !busy {
                    app.set_stopping(false);
                }
            }
            // The catalogue is bounded to 100 rows; discovery also covers a new
            // channel route, for which no selected-thread subscription exists yet.
            let refresh = observation
                .list_refreshed
                .is_none_or(|time| time.elapsed() >= Duration::from_secs(3));
            if refresh {
                observation.list_refreshed = Some(Instant::now());
            }
            drop(current);
            if refresh {
                refresh_chat(&app, state.clone(), true);
            }
        },
    );
}

/// Durable history uses the same single work-process fold as streaming. Batch
/// identities are retained for call ownership but never add a visible header.
pub(crate) fn project_history(turns: &[NativeChatTurn]) -> Vec<TimelineRow> {
    let mut rows = Vec::new();
    // Fold identity is model-wide: `toggle` folds by id, so no two batches or
    // processes may share one, not even across turns.
    let mut batch = 0i32;
    let mut process = 0i32;
    for turn in turns {
        // Queue rows are rendered by the composer. Their durable turn is only
        // an admission placeholder and must not become a second user bubble.
        if is_queue_only_turn(turn) {
            continue;
        }
        let mut user = history_user(&turn.user);
        // The bubble carries the turn identity: it is the anchor the reducer's
        // live rows are matched against when the durable snapshot takes over.
        user.id = format!("user-turn-{}", turn.root_id).into();
        rows.push(user);
        process += 1;
        let bar_at = rows.len();
        let mut bar = blank_row(
            crate::timeline::KIND_PROCESS,
            format!("process-{}", turn.root_id),
        );
        bar.payload = bar_at as i32;
        bar.process_idx = process;
        bar.process_root = turn.root_id.as_str().into();
        bar.text = crate::timeline_text::process_label(0).into();
        bar.visible = false;
        rows.push(bar);
        let mut answer_at = None;
        for round in &turn.rounds {
            let tools = round.items.len() as i32;
            let activity = !round.reasoning.is_empty() || tools > 0;
            if !activity {
                // A plain-text round has no batch: the answer alone carries it.
                if !round.text.is_empty() {
                    let mut body = blank_row(
                        crate::timeline::KIND_BODY,
                        format!("body-{}-{}", round.execution, round.round),
                    );
                    body.text = round.text.as_str().into();
                    body.blocks = crate::message_blocks::from_text(&round.text);
                    body.foldable = true;
                    if round.is_answer {
                        answer_at = Some(rows.len());
                    }
                    rows.push(body);
                }
                continue;
            }
            let group = batch;
            batch += 1;
            // The folded bar reads out the newest entry of its batch, exactly
            // as the live reducer stamps it.
            let bar_at = rows.len();
            let mut latest = slint::SharedString::default();
            let mut bar = blank_row(
                crate::timeline::KIND_GROUP,
                format!("group-{}-{}", round.execution, round.round),
            );
            bar.text = crate::timeline_text::tool_calls(tools).into();
            bar.payload = bar_at as i32;
            bar.group_idx = group;
            bar.visible = false;
            bar.group_open = true;
            bar.open = true;
            bar.foldable = true;
            rows.push(bar);
            if !round.reasoning.is_empty() {
                let mut reason = blank_row(
                    crate::timeline::KIND_REASON,
                    format!("reason-{}-{}", round.execution, round.round),
                );
                reason.tool_name = crate::timeline_text::thought_done().into();
                reason.summary = crate::message_blocks::reasoning_preview(&round.reasoning)
                    .as_str()
                    .into();
                latest = reason.summary.clone();
                reason.detail = round.reasoning.as_str().into();
                reason.payload = rows.len() as i32;
                reason.group_idx = group;
                reason.group_open = true;
                reason.foldable = true;
                rows.push(reason);
            }
            if !round.text.is_empty() {
                let mut body = blank_row(
                    crate::timeline::KIND_BODY,
                    format!("body-{}-{}", round.execution, round.round),
                );
                body.text = round.text.as_str().into();
                body.blocks = crate::message_blocks::from_text(&round.text);
                body.foldable = true;
                if round.is_answer {
                    answer_at = Some(rows.len());
                }
                rows.push(body);
            }
            for item in &round.items {
                let mut row = blank_row(crate::timeline::KIND_TOOL, format!("tool-{}", item.id));
                let label = crate::timeline_text::tool_label(&item.title);
                let (target, patch) = crate::timeline_text::tool_target(item);
                row.tool_name = label.as_str().into();
                row.tool_icon = crate::tool_icons::workflow_icon(&item.title).into();
                row.target = target.as_str().into();
                row.tool_metrics = crate::timeline_text::tool_metrics(item).into();
                row.tool_metrics_visible = !row.tool_metrics.is_empty();
                row.summary = crate::timeline_text::tool_summary(item).as_str().into();
                latest = row.summary.clone();
                row.detail = crate::timeline_text::copy_text(&item.sections, item.detail.as_str());
                row.sections = crate::timeline_text::sections(&item.sections);
                row.tokens = item.tokens.as_str().into();
                row.duration = item.duration.as_str().into();
                row.status_kind = crate::timeline::status_kind(&item.state);
                row.patch = if patch {
                    crate::timeline_text::patch_cards(&item.patch_files)
                } else {
                    ModelRc::default()
                };
                row.payload = rows.len() as i32;
                row.group_idx = group;
                row.group_open = true;
                row.foldable = true;
                rows.push(row);
            }
            rows[bar_at].summary = latest;
        }
        // Child runs render as cards at the turn's tail, the same place the web
        // messenger parks its subagent panel. The projection is shared with the
        // live reducer, so a card looks and behaves identically in both paths.
        for card in &turn.subagents {
            if card.session_id.is_empty() {
                continue;
            }
            let mut row = crate::timeline::subagent_row(card);
            row.payload = rows.len() as i32;
            rows.push(row);
        }
        // The metrics belong to the turn and ride on the body row that ends it,
        // which is also the row that owns the copy action: the same
        // owner the live reducer picks with its backward walk.
        if let Some(body) = answer_at {
            rows[body].stats = turn_stats(&turn.assistant);
        } else {
            let mut body = blank_row(
                crate::timeline::KIND_BODY,
                format!("status-{}", turn.root_id),
            );
            body.foldable = true;
            body.stats = turn_stats(&turn.assistant);
            answer_at = Some(rows.len());
            rows.push(body);
        }
        let end = rows.len();
        fold_history_process(
            rows.as_mut_slice(),
            bar_at,
            end,
            process,
            &turn.assistant.state,
            answer_at.unwrap_or(usize::MAX),
        );
        if turn.process_steps > 0 {
            rows[bar_at].text =
                crate::timeline_text::process_label(turn.process_steps.min(i32::MAX as i64) as i32)
                    .into();
            rows[bar_at].visible = true;
        }
    }
    crate::timeline_text::process_models(&mut rows);
    rows
}

fn is_queue_only_turn(turn: &NativeChatTurn) -> bool {
    let status = turn.assistant.stats_status.trim();
    let queued = status == "正在排队"
        || status == "任务已排队"
        || status.starts_with("正在排队 ·")
        || status.starts_with("任务已排队 ·");
    queued
        && turn.assistant.text.trim().is_empty()
        && turn.rounds.iter().all(|round| {
            round.reasoning.trim().is_empty()
                && round.text.trim().is_empty()
                && round.items.is_empty()
        })
        && turn.subagents.is_empty()
}

/// Apply the same single process fold as the live reducer, including actions
/// after the newest answer. A stopped turn need not have an answer at all.
fn fold_history_process(
    rows: &mut [TimelineRow],
    bar_at: usize,
    end: usize,
    process: i32,
    state: &str,
    newest: usize,
) {
    let mut steps = 0i32;
    for index in (bar_at + 1)..end {
        if rows[index].kind == crate::timeline::KIND_GROUP
            || crate::timeline::is_process_step(rows[index].kind, index, newest)
        {
            rows[index].process_idx = process;
            steps += i32::from(rows[index].kind != crate::timeline::KIND_GROUP);
        }
    }
    let label = crate::timeline_text::process_label(steps);
    let phase = match state {
        "completed" | "done" | "任务完成" => "已完成",
        "failed" | "error" | "rejected" | "system_error" | "执行失败" => "错误",
        "cancelled" | "stopped" | "interrupted" | "已停止" => "已停止",
        "queued" | "pending" | "正在排队" => "排队中",
        "waiting_input" | "waiting_user_input" | "等待用户输入" => "等待回答",
        "waiting_approval" | "等待确认" => "等待确认",
        _ => "正在处理…",
    };
    let mut activity = phase.to_string();
    if phase == "正在处理…" {
        for row in &rows[(bar_at + 1)..end] {
            if row.status_kind != crate::timeline::STATUS_RUNNING {
                continue;
            }
            if row.kind == crate::timeline::KIND_TOOL {
                activity = format!("执行工具：{}", row.tool_name);
            } else if row.kind == crate::timeline::KIND_REASON {
                activity = crate::timeline_text::thought_running().into();
            }
        }
    }
    let bar = &mut rows[bar_at];
    bar.status_kind = if phase == "正在处理…" {
        crate::timeline::STATUS_RUNNING
    } else if phase == "错误" {
        crate::timeline::STATUS_FAILED
    } else {
        crate::timeline::STATUS_DONE
    };
    bar.text = label.as_str().into();
    bar.summary = activity.into();
    bar.visible = steps > 0;
}

/// The turn's statistics footer, projected once per turn and attached to the
/// body row that carries the durable answer.
fn turn_stats(assistant: &wunder_desktop::NativeMessage) -> ModelRc<crate::TurnStatMetric> {
    ModelRc::new(VecModel::from(crate::timeline::stat_metrics(
        &crate::turn_stats::with_status(
            crate::turn_stats::with_user_round(
                crate::turn_stats::metrics_from_native(&assistant.stats),
                assistant.user_round,
            ),
            &assistant.stats_status,
        ),
    )))
}

fn history_user(message: &wunder_desktop::NativeMessage) -> TimelineRow {
    let mut row = blank_row(crate::timeline::KIND_USER, String::new());
    row.text = message.text.as_str().into();
    row.blocks = crate::message_blocks::from_text(&message.text);
    row.foldable = false;
    row
}

fn blank_row(kind: i32, id: String) -> TimelineRow {
    TimelineRow {
        kind,
        id: id.into(),
        visible: true,
        text: Default::default(),
        blocks: ModelRc::default(),
        summary: Default::default(),
        tool_name: Default::default(),
        tool_icon: Default::default(),
        target: Default::default(),
        tool_metrics: Default::default(),
        tool_metrics_visible: false,
        tokens: Default::default(),
        duration: Default::default(),
        detail: Default::default(),
        sections: ModelRc::default(),
        process_entries: ModelRc::default(),
        process_root: Default::default(),
        process_paged: false,
        process_local: false,
        process_older: false,
        process_newer: false,
        process_loading: false,
        process_error: false,
        process_revision: 0,
        status_kind: crate::timeline::STATUS_DONE,
        foldable: false,
        open: false,
        group_idx: crate::timeline::NO_GROUP,
        group_open: false,
        process_idx: crate::timeline::NO_PROCESS,
        process_open: false,
        payload: 0,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
        subagent_id: Default::default(),
        subagent_stoppable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::ModelRc;
    use std::time::Instant;
    use wunder_desktop::native::NativeChatRound;

    fn fixture(root: &str, text: &str, status: &str) -> NativeChatTurn {
        let message = wunder_desktop::NativeMessage {
            turn_id: root.into(),
            text: text.into(),
            mine: false,
            created_at: 0.0,
            state: status.into(),
            stats_status: status.into(),
            user_round: 0,
            stats: Default::default(),
        };
        NativeChatTurn {
            root_id: root.into(),
            user: wunder_desktop::NativeMessage {
                mine: true,
                text: "Fixture input".into(),
                ..message.clone()
            },
            assistant: message,
            rounds: vec![NativeChatRound {
                round: 1,
                execution: root.into(),
                is_answer: true,
                reasoning: String::new(),
                text: text.into(),
                items: Vec::new(),
            }],
            subagents: Vec::new(),
            process_steps: 0,
        }
    }

    /// Largest workspace file card the projections allow: `patch::project` keeps
    /// at most eight files and `timeline_text::patch_cards` at most 200 lines of
    /// each, so this is the real ceiling, not an invented one.
    fn near_limit_patch() -> Vec<wunder_desktop::NativePatchFile> {
        (0..8)
            .map(|file| wunder_desktop::NativePatchFile {
                path: format!("src/module_{file}.rs"),
                action: "update".into(),
                added: "200".into(),
                deleted: "200".into(),
                lines: (0..200)
                    .map(|line| wunder_desktop::NativePatchLine {
                        text: "    let value = compute(input_value);".into(),
                        kind: if line % 3 == 0 { "add" } else { "context" }.into(),
                        number: (line + 1).to_string(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Near-limit history: 50 turns (the observer's own cap) with 24 workflow
    /// entries each (`native_chat_turns` caps a turn at 24 entries) and one
    /// max-size patch card among them, plus a bounded answer body.
    fn near_limit_turns() -> Vec<NativeChatTurn> {
        let patch = near_limit_patch();
        let body = (0..40)
            .map(|line| format!("第 {line} 行输出内容，用于测量正文投影成本。"))
            .collect::<Vec<_>>()
            .join("\n\n");
        (0..50)
            .map(|index| {
                let mut turn = fixture(
                    &format!("fixture-turn-{index}"),
                    &format!("{body}\n\n结论：第 {index} 轮。"),
                    "任务完成",
                );
                turn.user.text = format!("第 {index} 轮输入").into();
                turn.rounds[0].items = (0..24)
                    .map(|slot| {
                        let base = NativeWorkflowEntry::from_payload(
                            &format!("fixture-tool-{index}-{slot}"),
                            "read_file",
                            "读取文件 · 完成\nsrc/module_0.rs\nFixture preview".into(),
                            "completed",
                            &serde_json::json!({"args":{"path":"src/module_0.rs"}}),
                        );
                        NativeWorkflowEntry {
                            patch_files: if slot % 12 == 0 {
                                patch.clone()
                            } else {
                                Vec::new()
                            },
                            ..base
                        }
                    })
                    .collect();
                turn
            })
            .collect()
    }

    /// A reloaded turn keeps its statistics: the projection reads the façade's
    /// resolved fields, and the footer it builds must match the one the live
    /// stream builds from the raw object.
    #[test]
    fn a_reloaded_turn_keeps_its_statistics_footer() {
        let mut turn = fixture("fixture-stats", "Fixture answer", "completed");
        turn.assistant.user_round = 57;
        turn.assistant.stats =
            wunder_desktop::native::NativeMessageStats::from_payload(&serde_json::json!({
                "interaction_duration_s": 12.44, "avg_model_round_speed_rounds": 2,
                "avg_model_round_speed_tps": 68234, "request_consumed_tokens": 4096,
                "toolCalls": 3, "contextTokens": 12345, "model_request_count": 4
            }));

        let rows = project_history(&[turn]);
        let metrics: Vec<(String, String)> = rows
            .iter()
            .find(|row| row.kind == crate::timeline::KIND_BODY)
            .map(|row| {
                row.stats
                    .iter()
                    .map(|metric| (metric.key.to_string(), metric.value.to_string()))
                    .collect()
            })
            .expect("a body row carries the metrics");
        assert_eq!(
            metrics,
            vec![
                ("hashtag".to_string(), "57".to_string()),
                ("stopwatch".to_string(), "12.4s".to_string()),
                ("gauge-high".to_string(), "68234.0 token/s".to_string()),
                ("layer-group".to_string(), "12.3k".to_string()),
                ("bolt".to_string(), "4.1k".to_string()),
                ("screwdriver-wrench".to_string(), "3".to_string()),
                ("coins".to_string(), "4".to_string()),
            ],
        );
    }

    /// A turn the runtime reported no statistics for must render no footer at
    /// all rather than a row of placeholders.
    #[test]
    fn a_reloaded_turn_without_statistics_has_no_footer() {
        let rows = project_history(&[fixture("fixture-plain", "Fixture answer", "completed")]);
        let count = rows
            .iter()
            .find(|row| row.kind == crate::timeline::KIND_BODY)
            .map(|row| row.stats.row_count())
            .expect("a body row");
        assert_eq!(count, 0, "no metrics renders no row");
    }

    #[test]
    fn process_only_body_does_not_take_the_final_reply_footer() {
        let mut turn = fixture("fixture-process-only", "", "running");
        turn.process_steps = 1;
        turn.rounds[0].text = "Fixture intermediate note".into();
        turn.rounds[0].is_answer = false;
        let rows = project_history(&[turn]);
        let intermediate = rows
            .iter()
            .find(|row| row.text == "Fixture intermediate note")
            .unwrap();
        assert_ne!(intermediate.process_idx, crate::timeline::NO_PROCESS);
        assert_eq!(intermediate.stats.row_count(), 0);
        let footer = rows.last().unwrap();
        assert!(footer.text.is_empty());
        assert_eq!(footer.process_idx, crate::timeline::NO_PROCESS);
        assert_eq!(footer.stats.row_data(0).unwrap().key, "status-running");
    }

    /// A reloaded turn rebuilds the entries the live stream showed: per model
    /// round one batch holding its thinking and its tools, then that round's
    /// answer block. Collapsing the turn to its newest answer alone is what made
    /// the timeline look emptied once a reply finished.
    #[test]
    fn a_reloaded_turn_keeps_every_rounds_entries() {
        let mut turn = fixture("fixture-rounds", "结论", "任务完成");
        turn.rounds = vec![
            NativeChatRound {
                round: 1,
                execution: "fixture-rounds".into(),
                is_answer: false,
                reasoning: "先想想第一步".into(),
                text: "第一步说明".into(),
                items: vec![NativeWorkflowEntry::from_payload(
                    "fixture-round-tool",
                    "read_file",
                    "读取文件 · 完成\nsrc/module_0.rs\nFixture preview".into(),
                    "completed",
                    &serde_json::json!({"args":{"path":"src/module_0.rs"}}),
                )],
            },
            NativeChatRound {
                round: 2,
                execution: "fixture-rounds".into(),
                is_answer: true,
                reasoning: "再想想第二步".into(),
                text: "结论".into(),
                items: Vec::new(),
            },
        ];
        let rows = project_history(&[turn]);
        let kinds: Vec<i32> = rows.iter().map(|row| row.kind).collect();
        assert_eq!(
            kinds,
            vec![
                crate::timeline::KIND_USER,
                crate::timeline::KIND_PROCESS,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_BODY,
                crate::timeline::KIND_TOOL,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_BODY,
            ],
        );
        // The bubble carries the turn identity the live tail is matched on.
        assert_eq!(rows[0].id, "user-turn-fixture-rounds");
        // Each round folds as its own batch, and thinking keeps its full text.
        assert_eq!(rows[2].group_idx, rows[3].group_idx);
        assert_eq!(rows[2].group_idx, rows[5].group_idx);
        assert_ne!(rows[2].group_idx, rows[6].group_idx);
        assert_eq!(rows[3].detail, "先想想第一步");
        assert_eq!(
            rows[3].tool_name.as_str(),
            crate::timeline_text::thought_done()
        );
        assert_eq!(rows[4].text, "第一步说明");
        assert_eq!(rows[8].text, "结论");
        // A fold handle always addresses its own row, or toggling would move a
        // different entry. A body block has no handle: for it `foldable` means
        // "frozen", which is what shows its metrics and actions line.
        for row in rows.iter().filter(|row| {
            matches!(
                row.kind,
                crate::timeline::KIND_GROUP
                    | crate::timeline::KIND_REASON
                    | crate::timeline::KIND_TOOL
                    | crate::timeline::KIND_PROCESS
            )
        }) {
            assert_eq!(
                rows[row.payload as usize].id, row.id,
                "a foldable row must point at itself"
            );
        }
        // The turn's process bar counts every step before the newest answer:
        // two thinking segments, one intermediate note and one tool call. The
        // batches inside it are stamped open — unfolding the process is the
        // request to see them — while the newest answer stays outside the fold.
        let process = rows[1].process_idx;
        assert_ne!(process, crate::timeline::NO_PROCESS);
        assert_eq!(rows[1].text, crate::timeline_text::process_label(4));
        assert_eq!(rows[1].summary, "已完成");
        assert!(rows[1].visible, "a real process earns its bar");
        assert!(!rows[1].open && !rows[1].process_open, "it ships folded");
        for row in &rows[2..8] {
            assert_eq!(row.process_idx, process, "step {row:?} escaped the fold");
            if row.group_idx != crate::timeline::NO_GROUP {
                assert!(row.group_open, "the process is the only fold gate");
            }
        }
        assert_eq!(
            rows[8].process_idx,
            crate::timeline::NO_PROCESS,
            "the newest answer block stays outside the fold"
        );
    }

    /// §12.2 evidence: the bounded worst case is measured, not estimated. Both
    /// phases are bounded by the projections themselves, so this is the whole
    /// cost the UI thread pays before Slint sees a single row.
    #[test]
    fn worst_case_history_projection_and_unfold_stay_bounded() {
        let started = Instant::now();
        let turns = near_limit_turns();
        let built = started.elapsed();

        let started = Instant::now();
        let rows = project_history(&turns);
        let projected = started.elapsed();

        let mut timeline = crate::timeline::Timeline::new();
        let started = Instant::now();
        timeline.set_history(rows.clone());
        let published = started.elapsed();

        // Before the user touches anything, each turn reads as its bubble, one
        // process bar and its newest answer: 50 turns x 3 laid-out rows.
        let entries_per_turn = turns[0].rounds[0].items.len();
        let laid_out = |model: &slint::ModelRc<TimelineRow>| {
            model
                .iter()
                .filter(|row| {
                    row.visible
                        // The two gates `TimelineRowView.visible` applies.
                        && (row.kind == crate::timeline::KIND_GROUP
                            || row.kind == crate::timeline::KIND_PROCESS
                            || row.group_idx == crate::timeline::NO_GROUP
                            || row.group_open)
                        && (row.kind == crate::timeline::KIND_PROCESS
                            || row.process_idx == crate::timeline::NO_PROCESS
                            || row.process_open)
                })
                .count()
        };
        assert_eq!(
            laid_out(&timeline.model()),
            50 * 3,
            "the default column is one process line and one answer per turn",
        );

        // Reach the fully opened column the way a user does: one handle at a
        // time. Every toggle is a bounded scan, so the whole history opens in
        // bounded clicks.
        let started = Instant::now();
        let mut opened = 0usize;
        for row in rows.iter() {
            if row.kind == crate::timeline::KIND_PROCESS && row.visible && !row.open {
                timeline.toggle(row.payload);
                opened += 1;
            }
        }
        let unfolded = started.elapsed();

        let rows = timeline.model();
        let exposed = laid_out(&rows);
        assert_eq!(opened, 50, "every turn's process ships folded");
        assert_eq!(
            exposed, 1350,
            "each process exposes its tools without an extra batch row",
        );
        let mut kinds = [0usize; 7];
        for row in rows.iter() {
            kinds[row.kind as usize] += 1;
        }
        println!(
            "timeline worst case: turns={} entries/turn={} rows={} exposed={} \
             build={built:?} project={projected:?} publish={published:?} unfold={unfolded:?}",
            turns.len(),
            entries_per_turn,
            rows.row_count(),
            exposed,
        );
        assert_eq!(kinds[0], 50, "one user bubble per turn");
        assert_eq!(kinds[1], 50, "one answer block per turn");
        assert_eq!(kinds[3], 1200, "24 tool entries x 50 turns");
        assert_eq!(kinds[4], 50, "one batch bar per turn");
        assert_eq!(kinds[6], 50, "one process bar per turn");
        // Rows are one flat vector: 50 turns x (1 bubble + 1 process bar +
        // 1 batch bar + 24 tool entries + 1 answer) = 1400, with no nesting or
        // duplication.
        assert_eq!(rows.row_count(), 1400);
        // The projection is O(rows); keep a loose ceiling so a future change that
        // makes it quadratic fails here instead of on the user's machine.
        assert!(
            projected.as_millis() < 4_000,
            "worst-case projection took {projected:?}",
        );
        // Patch cards are the most expensive single projection, so measure them
        // on their own instead of inferring their cost from the whole history: at
        // this row count the difference is inside the run-to-run noise.
        let cards = turns
            .iter()
            .flat_map(|turn| turn.rounds.iter().flat_map(|round| round.items.iter()))
            .filter(|entry| !entry.patch_files.is_empty())
            .count();
        let started = Instant::now();
        let mut lines = 0usize;
        for turn in &turns {
            for entry in turn.rounds.iter().flat_map(|round| round.items.iter()) {
                for card in crate::timeline_text::patch_cards(&entry.patch_files).iter() {
                    lines += card.lines.row_count();
                }
            }
        }
        let cards_ms = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "timeline patch projection: cards={cards} rendered_lines={lines} \
             rebuilt_once={cards_ms:.1}ms",
        );
        // 50 turns x 24 entries x 8 files x 200 lines is the projection ceiling;
        // a full rebuild is bounded and must stay far away from a frame budget.
        assert!(lines > 0 && cards > 0, "the fixture must carry patch cards");
        assert!(
            cards_ms < 2_000.0,
            "rebuilding every patch card took {cards_ms}ms",
        );
    }

    /// The shipped shape of a finished turn: the send path keys the live turn by
    /// session while storage keys it by its root message, so the two never share
    /// an id. Handing the turn over on that difference is what left a second
    /// bubble on screen after the last reply.
    #[test]
    fn a_finished_live_turn_is_published_once() {
        let mut timeline = crate::timeline::Timeline::new();
        timeline.set_history(project_history(&[fixture(
            "fixture-first",
            "Earlier answer",
            "任务完成",
        )]));
        timeline.begin_turn("fixture-session", "Fixture input");
        timeline.start_body(1);
        timeline.append_body("Latest answer").unwrap();
        timeline.flush();
        timeline.finish(false);

        // Storage commits the turn the live builder just settled.
        timeline.publish_history(project_history(&[
            fixture("fixture-first", "Earlier answer", "任务完成"),
            fixture("fixture-session-root", "Latest answer", "任务完成"),
        ]));

        let rows = timeline.model();
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == crate::timeline::KIND_USER)
                .count(),
            2,
            "one bubble per turn, the finished one not twice"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.kind == crate::timeline::KIND_BODY)
                .count(),
            2,
            "one durable answer per turn"
        );
        drop(rows);
        assert_eq!(timeline.last_answer(), "Latest answer");
    }

    /// §12.2 evidence: the frame flush rewrites only the active tail block. Row
    /// handles of the frozen history must be the very same model instances
    /// before and after streaming, so no frame can rebuild the column.
    #[test]
    fn streaming_a_frame_rewrites_only_the_active_tail_block() {
        let rows = project_history(&near_limit_turns());
        let mut timeline = crate::timeline::Timeline::new();
        timeline.set_history(rows);

        let frozen = timeline.model();
        let before: Vec<(i32, ModelRc<crate::TextBlock>, ModelRc<crate::PatchCard>)> = frozen
            .iter()
            .map(|row| (row.kind, row.blocks.clone(), row.patch.clone()))
            .collect();

        timeline.begin_turn("fixture-live", "Fixture input");
        timeline.start_body(1);
        // Header cost of one live turn: user bubble + process bar + first body
        // block. Captured rather than guessed, so a reducer change that adds a
        // row shows up as a deliberate update here instead of a silent drift.
        let after_header = timeline.model().row_count();
        assert_eq!(
            after_header,
            before.len() + 3,
            "a live turn header is bubble + process bar + body block",
        );
        let mut frames = 0usize;
        let started = Instant::now();
        for index in 0..40 {
            timeline
                .append_body(&format!("第 {index} 帧增量\n"))
                .unwrap();
            timeline.flush();
            frames += 1;
        }
        let streamed = started.elapsed();

        let after = timeline.model();
        // Streaming must not add rows at all: every frame rewrites the body
        // block it already owns, so row count is frozen at the header cost.
        assert_eq!(
            after.row_count(),
            after_header,
            "streaming a frame appended rows instead of rewriting the tail block",
        );
        for (index, (kind, blocks, patch)) in before.iter().enumerate() {
            let row = after.row_data(index).expect("frozen row kept");
            assert_eq!(row.kind, *kind);
            assert_eq!(
                &row.blocks, blocks,
                "row {index} was re-projected by a stream frame",
            );
            assert_eq!(
                &row.patch, patch,
                "row {index} rebuilt its patch card on a stream frame",
            );
        }
        // 40 frames with no new text must do no work at all: only the frames
        // that carried a delta re-project the tail block.
        let started = Instant::now();
        let tail = after.row_data(after.row_count() - 1).unwrap();
        for _ in 0..1_000 {
            timeline.flush();
        }
        let idle = started.elapsed();
        let settled = timeline.model().row_data(after.row_count() - 1).unwrap();
        assert_eq!(
            settled.blocks, tail.blocks,
            "an idle frame re-projected the tail block",
        );
        println!(
            "timeline streaming: {frames} frames with deltas in {streamed:?}, \
             1000 idle flushes in {idle:?}, tail text {} bytes",
            settled.text.len(),
        );
    }

    #[test]
    fn history_keeps_every_turn_laid_out_without_a_process_to_fold() {
        // Both turns are plain text: bubble + answer, so their process bars
        // publish invisible rather than growing a shell around nothing.
        let rows = project_history(&[
            fixture("fixture-old", "Earlier answer", "任务完成"),
            fixture("fixture-new", "Latest answer", "任务完成"),
        ]);
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[0].id, "user-turn-fixture-old");
        assert_eq!(rows[1].kind, crate::timeline::KIND_PROCESS);
        assert!(!rows[1].visible, "a turn with no process shows no bar");
        assert_eq!(rows[1].text, crate::timeline_text::process_label(0));
        assert_eq!(rows[2].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[2].process_idx, crate::timeline::NO_PROCESS);
        assert_eq!(rows[3].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[3].id, "user-turn-fixture-new");
        assert_eq!(rows[4].kind, crate::timeline::KIND_PROCESS);
        assert!(!rows[4].visible);
        assert_eq!(rows[5].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[5].text, "Latest answer");
        for row in rows
            .iter()
            .filter(|row| row.kind != crate::timeline::KIND_PROCESS)
        {
            assert!(row.visible, "no content row is hidden");
        }
        // The metrics ride on the newest answer.
        assert_eq!(rows[2].stats.row_count(), 0, "no metrics renders no footer");
    }

    #[test]
    fn history_keeps_the_compaction_entry_of_a_failed_turn() {
        let mut turn = fixture("fixture-compact", "", "执行失败");
        turn.rounds[0].items.push(NativeWorkflowEntry::from_payload(
            "fixture-compact-item",
            "上下文压缩",
            String::new(),
            "failed",
            &serde_json::json!({"status":"failed", "error_message":"fixture failure"}),
        ));
        let rows = project_history(&[turn]);
        // A failed turn without an answer still uses the same process handle.
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[1].kind, crate::timeline::KIND_PROCESS);
        assert!(rows[1].visible);
        assert_eq!(rows[2].kind, crate::timeline::KIND_GROUP);
        assert_eq!(rows[2].text, "执行工具 1 次");
        assert!(!rows[2].visible, "ownership batch stays internal");
        assert_eq!(rows[3].kind, crate::timeline::KIND_TOOL);
        assert_eq!(rows[3].status_kind, crate::timeline::STATUS_FAILED);
        assert_eq!(rows[3].process_idx, rows[1].process_idx);
        assert!(rows[3].detail.contains("fixture failure"));
        assert_eq!(rows[4].stats.row_data(0).unwrap().key, "status-error");
    }
}
