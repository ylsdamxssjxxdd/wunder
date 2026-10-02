//! Passive native updates are independent of locally submitted executions.
use super::*;
use std::collections::HashMap;
use wunder_desktop::native::{NativeChatTurn, NativeThreadUpdate, NativeThreadWatch};

#[derive(Default)]
pub(super) struct Observation {
    timer: Timer,
    session: String,
    watch: Option<NativeThreadWatch>,
    model: Rc<VecModel<ChatTurn>>,
    cached: HashMap<String, NativeChatTurn>,
    blocks: HashMap<String, Blocks>,
    list_refreshed: Option<Instant>,
    ready: bool,
}

impl Observation {
    pub(super) fn detach(&mut self) {
        self.ready = false;
        self.watch = None;
        self.session.clear();
        self.cached.clear();
        self.blocks.clear();
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
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
            app.set_observing_thread(true);
            let desktop = current.desktop.clone();
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
                        observation.cached.clear();
                        observation.blocks.clear();
                        observation.model = Rc::new(VecModel::default());
                        for turn in turns {
                            apply_turn(&app, observation, turn);
                        }
                        app.set_turns(ModelRc::from(observation.model.clone()));
                        dirty = true;
                    }
                    NativeThreadUpdate::Turns(turns) => {
                        for turn in turns {
                            dirty |= apply_turn(&app, observation, turn);
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
            if dirty {
                let busy = observation.cached.values().any(|turn| {
                    matches!(
                        turn.assistant.stats_status.as_str(),
                        "正在生成…" | "正在排队"
                    ) || turn.assistant.stats_status.starts_with("正在排队 ·")
                });
                app.set_busy(busy);
                if !busy {
                    app.set_stopping(false);
                }
                if app.get_follow_output() {
                    app.set_scroll_revision(app.get_scroll_revision().wrapping_add(1));
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

fn apply_turn(app: &MainWindow, observation: &mut Observation, turn: NativeChatTurn) -> bool {
    match patch_turn(
        observation,
        turn,
        agent_avatar_glyph(app),
        agent_avatar_tone(app),
    ) {
        Ok(changed) => changed,
        Err(error) => {
            app.set_status(error.into());
            false
        }
    }
}

fn patch_turn(
    observation: &mut Observation,
    turn: NativeChatTurn,
    glyph: slint::SharedString,
    tone: i32,
) -> Result<bool, String> {
    if observation.cached.get(&turn.root_id) == Some(&turn) {
        return Ok(false);
    }
    let index = observation
        .model
        .iter()
        .position(|row| row.root_id == turn.root_id);
    let old = index.and_then(|index| observation.model.row_data(index));
    let blocks = observation
        .blocks
        .entry(turn.root_id.clone())
        .or_insert_with(Blocks::new);
    if blocks.raw != turn.assistant.text {
        blocks.replace(&turn.assistant.text)?;
        blocks.flush();
    }
    let assistant_blocks = ModelRc::from(blocks.model.clone());
    let map =
        |message: &wunder_desktop::NativeMessage, previous: Option<&ChatMessage>| ChatMessage {
            workflow: !message.workflow_detail.is_empty(),
            workflow_detail: message.workflow_detail.as_str().into(),
            text: message.text.as_str().into(),
            mine: message.mine,
            time: format_time(message.created_at).into(),
            state: message.state.as_str().into(),
            stats_status: message.stats_status.as_str().into(),
            stats_duration: message.stats_duration.as_str().into(),
            stats_speed: message.stats_speed.as_str().into(),
            stats_context: message.stats_context.as_str().into(),
            stats_quota: message.stats_quota.as_str().into(),
            stats_tools: message.stats_tools.as_str().into(),
            stats_credits: message.stats_credits.as_str().into(),
            blocks: if !message.mine {
                assistant_blocks.clone()
            } else {
                previous
                    .filter(|row| row.text == message.text)
                    .map(|row| row.blocks.clone())
                    .unwrap_or_else(|| crate::message_blocks::from_text(&message.text))
            },
            avatar_glyph: glyph.clone(),
            avatar_tone: tone,
        };
    let mut row = ChatTurn {
        root_id: turn.root_id.as_str().into(),
        user: map(&turn.user, old.as_ref().map(|row| &row.user)),
        assistant: map(&turn.assistant, old.as_ref().map(|row| &row.assistant)),
    };
    row.assistant.blocks = assistant_blocks;
    observation.cached.insert(turn.root_id.clone(), turn);
    if let Some(index) = index {
        observation.model.set_row_data(index, row);
    } else {
        if observation.model.row_count() >= 50 {
            if let Some(old) = observation.model.row_data(0) {
                observation.cached.remove(old.root_id.as_str());
                observation.blocks.remove(old.root_id.as_str());
            }
            observation.model.remove(0);
        }
        observation.model.push(row);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(root: &str, text: &str, status: &str) -> NativeChatTurn {
        let message = wunder_desktop::NativeMessage {
            turn_id: root.into(),
            text: text.into(),
            workflow_detail: String::new(),
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
        }
    }

    #[test]
    fn passive_turns_keep_pairs_and_reuse_history_and_stream_blocks() {
        let mut observation = Observation::default();
        let first = fixture("fixture-first", "Settled", "任务完成");
        patch_turn(&mut observation, first.clone(), "*".into(), 1).unwrap();
        let history = observation.model.row_data(0).unwrap().assistant.blocks;
        let next = fixture("fixture-next", "Partial\n", "正在生成…");
        patch_turn(&mut observation, next.clone(), "*".into(), 1).unwrap();
        let blocks = observation.model.row_data(1).unwrap().assistant.blocks;
        assert!(!patch_turn(&mut observation, next, "*".into(), 1).unwrap());
        patch_turn(
            &mut observation,
            fixture("fixture-next", "Partial\nResult", "任务完成"),
            "*".into(),
            1,
        )
        .unwrap();
        assert_eq!(observation.model.row_count(), 2);
        let settled = observation.model.row_data(0).unwrap();
        assert_eq!(settled.assistant.stats_status, "任务完成");
        assert_eq!(settled.assistant.blocks, history);
        let completed = observation.model.row_data(1).unwrap();
        assert_eq!(completed.assistant.blocks, blocks);
        assert_eq!(completed.assistant.text, "Partial\nResult");
        assert!(completed.user.mine && !completed.assistant.mine);
        for index in 0..60 {
            patch_turn(
                &mut observation,
                fixture(&format!("fixture-{index}"), "Result", "任务完成"),
                "*".into(),
                1,
            )
            .unwrap();
        }
        assert_eq!(observation.model.row_count(), 50);
        assert_eq!(observation.cached.len(), 50);
        assert_eq!(observation.blocks.len(), 50);
    }
}
