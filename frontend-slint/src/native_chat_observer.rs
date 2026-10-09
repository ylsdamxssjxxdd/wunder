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

    /// Publish the durable history into the shared model without touching the
    /// live turn that is already appended to it.
    fn publish(&self, app: &MainWindow, timeline: &Rc<RefCell<crate::timeline::Timeline>>) {
        let mut live = timeline.borrow_mut();
        live.publish_history(self.rows.clone());
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
            if current.active.is_some() || app.get_session_loading() || app.get_creating_session() {
                return;
            }
            let desktop = current.desktop.clone();
            // Cloned before the observation borrow: the history rows and a live
            // turn share one model.
            let timeline = current.timeline.clone();
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

/// Durable history as timeline rows, in the shape the live reducer produces: one
/// folded divider per completed turn, its user bubble, then per model round a
/// batch (bar, thinking, tool entries) and that round's answer block. Thinking
/// and every round's text are part of the durable snapshot, so a reloaded turn
/// keeps the entries it showed while it streamed. The newest turn carries its
/// divider hidden and everything unfolded.
pub(crate) fn project_history(turns: &[NativeChatTurn]) -> Vec<TimelineRow> {
    let last = turns.len().saturating_sub(1);
    let mut rows = Vec::new();
    // Batch identity is model-wide: `toggle` folds by `group_idx`, so no two
    // batches may share one, not even across turns.
    let mut batch = 0i32;
    for (index, turn) in turns.iter().enumerate() {
        let current = index == last;
        let turn_start = rows.len();
        let mut divider = blank_row(
            crate::timeline::KIND_DIVIDER,
            format!("turn-{}", turn.root_id),
        );
        divider.visible = !current;
        divider.foldable = true;
        divider.payload = turn_start as i32;
        rows.push(divider);
        rows.push(history_user(&turn.user));
        for round in &turn.rounds {
            let group = batch;
            batch += 1;
            let tools = round.items.len() as i32;
            let activity = !round.reasoning.is_empty() || tools > 0;
            // The folded bar reads out the newest entry of its batch, exactly as
            // the live reducer stamps it.
            let mut bar_at: Option<usize> = None;
            let mut latest = slint::SharedString::default();
            if activity {
                let mut bar = blank_row(
                    crate::timeline::KIND_GROUP,
                    format!("history-{index}-group-{}", round.round),
                );
                bar.text = crate::timeline_text::tool_calls(tools).into();
                // The newest turn reads exactly as it did while it streamed:
                // its batches start open. Older turns stay folded, both behind
                // their divider and behind their bars.
                bar.open = current;
                bar.payload = rows.len() as i32;
                bar.group_idx = group;
                bar.group_open = current;
                bar.visible = current;
                bar.foldable = true;
                bar_at = Some(rows.len());
                rows.push(bar);
            }
            if !round.reasoning.is_empty() {
                let mut reason = blank_row(
                    crate::timeline::KIND_REASON,
                    format!("history-{index}-reason-{}", round.round),
                );
                reason.tool_name = crate::timeline_text::thought_done().into();
                reason.summary =
                    crate::message_blocks::reasoning_preview(&round.reasoning).as_str().into();
                latest = reason.summary.clone();
                reason.detail = round.reasoning.as_str().into();
                reason.payload = rows.len() as i32;
                reason.group_idx = group;
                reason.group_open = current;
                reason.visible = current;
                reason.foldable = true;
                rows.push(reason);
            }
            if !round.text.is_empty() {
                let mut body = blank_row(
                    crate::timeline::KIND_BODY,
                    format!("history-{index}-body-{}", round.round),
                );
                body.text = round.text.as_str().into();
                body.blocks = crate::message_blocks::from_text(&round.text);
                body.foldable = true;
                body.visible = current;
                rows.push(body);
            }
            for item in &round.items {
                let mut row = blank_row(
                    crate::timeline::KIND_TOOL,
                    format!("tool-{}", item.id),
                );
                let label = crate::timeline_text::tool_label(&item.title);
                let (target, patch) = crate::timeline_text::target_of(
                    &item.title,
                    item.detail.as_str(),
                    &item.sections,
                );
                row.tool_name = label.as_str().into();
                row.tool_icon = crate::tool_icons::workflow_icon(&item.title).into();
                row.target = target.as_str().into();
                row.summary = crate::timeline_text::summary(item.detail.as_str()).as_str().into();
                latest = row.summary.clone();
                row.detail = crate::timeline_text::copy_text(&item.sections, item.detail.as_str());
                row.status_kind = crate::timeline::status_kind(&item.state);
                row.patch = if patch {
                    crate::timeline_text::patch_cards(&item.patch_files)
                } else {
                    ModelRc::default()
                };
                row.payload = rows.len() as i32;
                row.group_idx = group;
                row.group_open = current;
                row.visible = current;
                row.foldable = true;
                rows.push(row);
            }
            if let Some(at) = bar_at {
                rows[at].summary = latest;
            }
        }
        // The metrics belong to the turn and ride on the body row that ends it,
        // which is also the row that owns the copy and save actions: the same
        // owner the live reducer picks with its backward walk.
        if let Some(body) = (turn_start..rows.len()).rev().find(|index| {
            rows[*index].kind == crate::timeline::KIND_BODY
        }) {
            rows[body].stats = turn_stats(&turn.assistant);
        }
        if !current {
            // A completed turn folds behind its divider: the divider stays as
            // the fold handle, everything else of that turn is not laid out.
            for row in &mut rows[turn_start + 1..] {
                row.visible = false;
            }
        }
    }
    rows
}

/// The turn's statistics footer, projected once per turn and attached to the
/// body row that carries the durable answer.
fn turn_stats(assistant: &wunder_desktop::NativeMessage) -> ModelRc<crate::TurnStatMetric> {
    ModelRc::new(VecModel::from(crate::timeline::stat_metrics(
        &crate::turn_stats::metrics_from_parts(
            assistant.stats_duration.as_str(),
            assistant.stats_speed.as_str(),
            assistant.stats_context.as_str(),
            assistant.stats_quota.as_str(),
            assistant.stats_tools.as_str(),
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
        detail: Default::default(),
        status_kind: crate::timeline::STATUS_DONE,
        foldable: false,
        open: false,
        group_idx: crate::timeline::NO_GROUP,
        group_open: false,
        payload: 0,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
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
            stats_duration: String::new(),
            stats_speed: String::new(),
            stats_context: String::new(),
            stats_quota: String::new(),
            stats_tools: String::new(),
            stats_credits: String::new(),
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
                reasoning: String::new(),
                text: text.into(),
                items: Vec::new(),
            }],
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
        turn.assistant.stats_duration = "12.4s".into();
        turn.assistant.stats_speed = "68234.0/s".into();
        turn.assistant.stats_quota = "4.1k".into();
        turn.assistant.stats_tools = "3".into();
        turn.assistant.stats_context = "12345".into();

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
                ("stopwatch".to_string(), "12.4s".to_string()),
                ("gauge-high".to_string(), "68.2k/s".to_string()),
                ("layer-group".to_string(), "12.3k".to_string()),
                ("bolt".to_string(), "4.1k".to_string()),
                ("screwdriver-wrench".to_string(), "3".to_string()),
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
                crate::timeline::KIND_DIVIDER,
                crate::timeline::KIND_USER,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_BODY,
                crate::timeline::KIND_TOOL,
                crate::timeline::KIND_GROUP,
                crate::timeline::KIND_REASON,
                crate::timeline::KIND_BODY,
            ],
        );
        // The divider carries the turn identity the live tail is matched on.
        assert_eq!(rows[0].id, "turn-fixture-rounds");
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
                crate::timeline::KIND_DIVIDER
                    | crate::timeline::KIND_GROUP
                    | crate::timeline::KIND_REASON
                    | crate::timeline::KIND_TOOL
            )
        }) {
            assert_eq!(
                rows[row.payload as usize].id, row.id,
                "a foldable row must point at itself"
            );
        }
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

        // Worst honest case: unfold every completed turn, so nothing is skipped
        // by the visibility filter. A folded turn is represented by a *visible*
        // divider while everything it owns stays hidden, so those visible
        // dividers are the fold handles the user clicks.
        let folded: Vec<i32> = rows
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_DIVIDER && row.visible)
            .map(|row| row.payload)
            .collect();
        let started = Instant::now();
        let mut opened = 0usize;
        for divider in &folded {
            timeline.toggle(*divider);
            opened += 1;
        }
        let unfolded = started.elapsed();

        let rows = timeline.model();
        let exposed = rows.iter().filter(|row| row.visible).count();
        let hidden: Vec<i32> = rows
            .iter()
            .filter(|row| !row.visible)
            .map(|row| row.kind)
            .collect();
        let hidden_dividers = hidden
            .iter()
            .filter(|kind| **kind == crate::timeline::KIND_DIVIDER)
            .count();
        let mut kinds = [0usize; 6];
        for row in rows.iter() {
            kinds[row.kind as usize] += 1;
        }
        let entries_per_turn = turns[0].rounds[0].items.len();
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
        assert_eq!(kinds[5], 50, "one turn divider per turn");
        // Rows are one flat vector: 50 turns x (1 bubble + 1 body + 1 batch bar
        // + 24 tool entries + 1 divider) = 1400, with no nesting or duplication.
        assert_eq!(rows.row_count(), 1400);
        // Opening turns past the reducer's limit folds the oldest ones back, so
        // unfolding the whole history must not expose every row it owns: that is
        // exactly the regression this guards against. A folded-back turn costs
        // one invisible divider, and the live turn's divider is the other one.
        assert!(
            hidden_dividers > 1,
            "the cap must have folded turns back, leaving their dividers hidden",
        );
        // The hidden rows are the turns that were folded back plus the live
        // turn's divider, so they carry every kind a turn is made of.
        assert!(
            hidden.contains(&crate::timeline::KIND_USER)
                && hidden.contains(&crate::timeline::KIND_TOOL)
                && hidden.contains(&crate::timeline::KIND_BODY),
            "the rows folded back must be whole turns, not stray entries",
        );
        let per_turn = entries_per_turn + 3;
        let open_turns = rows
            .iter()
            .filter(|row| row.kind == crate::timeline::KIND_DIVIDER && row.open)
            .count();
        let max_open = 8;
        assert_eq!(
            open_turns, max_open,
            "the cap must leave exactly its limit of turns open",
        );
        assert!(
            exposed <= max_open * (per_turn + 1) + per_turn,
            "opening every turn laid out {exposed} rows; the cap must hold it near \
             {max_open} turns",
        );
        assert!(
            exposed < rows.row_count(),
            "an open history must still fold most of its rows",
        );
        // The turns that are open are open completely: the reducer never hides a
        // row inside a turn it reports as open.
        assert!(
            exposed >= per_turn,
            "at least the live turn must be fully laid out",
        );
        assert!(
            opened > 0,
            "the fixture must start with folded turns or this proves nothing",
        );
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
                .filter(|row| row.kind == crate::timeline::KIND_DIVIDER)
                .count(),
            2,
            "the live divider goes with the live copy"
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
        // Header cost of one live turn: divider + user bubble + first body
        // block. Captured rather than guessed, so a reducer change that adds a
        // row shows up as a deliberate update here instead of a silent drift.
        let after_header = timeline.model().row_count();
        assert_eq!(
            after_header,
            before.len() + 3,
            "a live turn header is divider + bubble + body block",
        );
        let mut frames = 0usize;
        let started = Instant::now();
        for index in 0..40 {
            timeline.append_body(&format!("第 {index} 帧增量\n")).unwrap();
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
    fn history_folds_older_turns_and_keeps_the_latest_open() {
        let rows = project_history(&[
            fixture("fixture-old", "Earlier answer", "任务完成"),
            fixture("fixture-new", "Latest answer", "任务完成"),
        ]);
        assert_eq!(rows.len(), 6);
        // The folded turn keeps its divider as the only laid-out row.
        assert_eq!(rows[0].kind, crate::timeline::KIND_DIVIDER);
        assert!(rows[0].visible);
        assert_eq!(rows[0].payload, 0);
        assert_eq!(rows[1].kind, crate::timeline::KIND_USER);
        assert!(!rows[1].visible);
        assert_eq!(rows[2].kind, crate::timeline::KIND_BODY);
        assert!(!rows[2].visible);
        assert_eq!(rows[3].kind, crate::timeline::KIND_DIVIDER);
        assert!(!rows[3].visible, "the newest turn needs no divider");
        assert_eq!(rows[4].kind, crate::timeline::KIND_USER);
        assert!(rows[4].visible);
        assert_eq!(rows[5].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[5].text, "Latest answer");
    }

    #[test]
    fn history_keeps_the_compaction_entry_of_a_failed_turn() {
        let mut turn = fixture("fixture-compact", "", "执行失败");
        turn.rounds[0]
            .items
            .push(NativeWorkflowEntry::from_payload(
                "fixture-compact-item",
                "上下文压缩",
                String::new(),
                "failed",
                &serde_json::json!({"status":"failed", "error_message":"fixture failure"}),
            ));
        let rows = project_history(&[turn]);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].kind, crate::timeline::KIND_DIVIDER);
        assert_eq!(rows[1].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[2].kind, crate::timeline::KIND_GROUP);
        assert_eq!(rows[2].text, "执行工具 1 次");
        assert_eq!(rows[3].kind, crate::timeline::KIND_TOOL);
        assert_eq!(rows[3].status_kind, crate::timeline::STATUS_FAILED);
        assert!(rows[3].detail.contains("fixture failure"));
    }
}
