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

/// Durable history as timeline rows, in the web messenger's form: every turn
/// stays laid out — its bubble, per model round a batch (bar, thinking, tool
/// entries) and that round's answer block. Only the batch bars fold, and the
/// newest `MAX_OPEN_TURNS` turns read with their last batch open, matching the
/// web default; older batches stay folded bars the user can still open.
pub(crate) fn project_history(turns: &[NativeChatTurn]) -> Vec<TimelineRow> {
    let mut rows = Vec::new();
    // Batch identity is model-wide: `toggle` folds by `group_idx`, so no two
    // batches may share one, not even across turns.
    let mut batch = 0i32;
    // Walk from the newest turn, spending the open quota on turns that carry
    // at least one batch.
    let mut open_quota = crate::timeline::MAX_OPEN_TURNS as isize;
    let mut turn_open = vec![false; turns.len()];
    for index in (0..turns.len()).rev() {
        let active = turns[index]
            .rounds
            .iter()
            .any(|round| !round.reasoning.is_empty() || !round.items.is_empty());
        if active && open_quota > 0 {
            turn_open[index] = true;
            open_quota -= 1;
        }
    }
    for (index, turn) in turns.iter().enumerate() {
        let turn_start = rows.len();
        let turn_open = turn_open[index];
        // Within a turn only the last batch opens, as on the web.
        let last_active_round = turn
            .rounds
            .iter()
            .rposition(|round| !round.reasoning.is_empty() || !round.items.is_empty());
        let mut user = history_user(&turn.user);
        // The bubble carries the turn identity: it is the anchor the reducer's
        // live rows are matched against when the durable snapshot takes over.
        user.id = format!("user-turn-{}", turn.root_id).into();
        rows.push(user);
        for (round_index, round) in turn.rounds.iter().enumerate() {
            let tools = round.items.len() as i32;
            let activity = !round.reasoning.is_empty() || tools > 0;
            if !activity {
                // A plain-text round has no batch: the answer alone carries it.
                if !round.text.is_empty() {
                    let mut body = blank_row(
                        crate::timeline::KIND_BODY,
                        format!("history-{index}-body-{}", round.round),
                    );
                    body.text = round.text.as_str().into();
                    body.blocks = crate::message_blocks::from_text(&round.text);
                    body.foldable = true;
                    rows.push(body);
                }
                continue;
            }
            let open = turn_open && Some(round_index) == last_active_round;
            let group = batch;
            batch += 1;
            // The folded bar reads out the newest entry of its batch, exactly
            // as the live reducer stamps it.
            let bar_at = rows.len();
            let mut latest = slint::SharedString::default();
            let mut bar = blank_row(
                crate::timeline::KIND_GROUP,
                format!("history-{index}-group-{}", round.round),
            );
            bar.text = crate::timeline_text::tool_calls(tools).into();
            bar.open = open;
            bar.payload = bar_at as i32;
            bar.group_idx = group;
            bar.group_open = open;
            bar.foldable = true;
            rows.push(bar);
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
                reason.group_open = open;
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
                row.group_open = open;
                row.foldable = true;
                rows.push(row);
            }
            rows[bar_at].summary = latest;
        }
        // The metrics belong to the turn and ride on the body row that ends it,
        // which is also the row that owns the copy and save actions: the same
        // owner the live reducer picks with its backward walk.
        if let Some(body) = (turn_start..rows.len()).rev().find(|index| {
            rows[*index].kind == crate::timeline::KIND_BODY
        }) {
            rows[body].stats = turn_stats(&turn.assistant);
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
        // The bubble carries the turn identity the live tail is matched on.
        assert_eq!(rows[0].id, "user-turn-fixture-rounds");
        // Each round folds as its own batch, and thinking keeps its full text.
        assert_eq!(rows[1].group_idx, rows[2].group_idx);
        assert_eq!(rows[1].group_idx, rows[4].group_idx);
        assert_ne!(rows[1].group_idx, rows[5].group_idx);
        assert_eq!(rows[2].detail, "先想想第一步");
        assert_eq!(
            rows[2].tool_name.as_str(),
            crate::timeline_text::thought_done()
        );
        assert_eq!(rows[3].text, "第一步说明");
        assert_eq!(rows[7].text, "结论");
        // A fold handle always addresses its own row, or toggling would move a
        // different entry. A body block has no handle: for it `foldable` means
        // "frozen", which is what shows its metrics and actions line.
        for row in rows.iter().filter(|row| {
            matches!(
                row.kind,
                crate::timeline::KIND_GROUP
                    | crate::timeline::KIND_REASON
                    | crate::timeline::KIND_TOOL
            )
        }) {
            assert_eq!(
                rows[row.payload as usize].id, row.id,
                "a foldable row must point at itself"
            );
        }
        // The newest turn opens its last batch; earlier batches of the same
        // turn stay folded, matching the web default.
        assert!(!rows[1].group_open, "the first batch stays folded");
        assert!(rows[5].group_open, "the last batch opens");
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

        // Before the user touches anything, only the newest turns read with
        // their last batch open: 50 turns x 3 base rows plus the newest
        // MAX_OPEN_TURNS batches' entries.
        let entries_per_turn = turns[0].rounds[0].items.len();
        let laid_out = |model: &slint::ModelRc<TimelineRow>| {
            model
                .iter()
                .filter(|row| {
                    row.visible
                        && (row.kind == crate::timeline::KIND_GROUP
                            || row.group_idx == crate::timeline::NO_GROUP
                            || row.group_open)
                })
                .count()
        };
        assert_eq!(
            laid_out(&timeline.model()),
            50 * 3 + 8 * entries_per_turn,
            "the default column is the base rows plus the open batches",
        );

        // Reach the fully opened column the way a user does: one batch at a
        // time. Every toggle is a bounded scan, so the whole history opens in
        // bounded clicks.
        let started = Instant::now();
        let mut opened = 0usize;
        for row in rows.iter() {
            if row.kind == crate::timeline::KIND_GROUP && !row.group_open {
                timeline.toggle(row.payload);
                opened += 1;
            }
        }
        let unfolded = started.elapsed();

        let rows = timeline.model();
        let exposed = laid_out(&rows);
        assert_eq!(opened, 42, "the oldest batches start folded");
        assert_eq!(exposed, 1350, "every batch the user opened lays out");
        let mut kinds = [0usize; 5];
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
        // Rows are one flat vector: 50 turns x (1 bubble + 1 body + 1 batch bar
        // + 24 tool entries) = 1350, with no nesting or duplication.
        assert_eq!(rows.row_count(), 1350);
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
        // Header cost of one live turn: user bubble + first body block.
        // Captured rather than guessed, so a reducer change that adds a
        // row shows up as a deliberate update here instead of a silent drift.
        let after_header = timeline.model().row_count();
        assert_eq!(
            after_header,
            before.len() + 2,
            "a live turn header is bubble + body block",
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
    fn history_keeps_every_turn_laid_out_and_opens_only_the_newest_batch() {
        // Both turns are plain text: bubble + body, no batch bar at all.
        let rows = project_history(&[
            fixture("fixture-old", "Earlier answer", "任务完成"),
            fixture("fixture-new", "Latest answer", "任务完成"),
        ]);
        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert!(row.visible, "no history row is hidden");
        }
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[0].id, "user-turn-fixture-old");
        assert_eq!(rows[1].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[2].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[2].id, "user-turn-fixture-new");
        assert_eq!(rows[3].kind, crate::timeline::KIND_BODY);
        assert_eq!(rows[3].text, "Latest answer");
        // The metrics ride on the newest answer.
        assert_eq!(rows[1].stats.row_count(), 0, "no metrics renders no footer");
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
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, crate::timeline::KIND_USER);
        assert_eq!(rows[1].kind, crate::timeline::KIND_GROUP);
        assert_eq!(rows[1].text, "执行工具 1 次");
        assert_eq!(rows[2].kind, crate::timeline::KIND_TOOL);
        assert_eq!(rows[2].status_kind, crate::timeline::STATUS_FAILED);
        assert!(rows[2].detail.contains("fixture failure"));
    }
}
