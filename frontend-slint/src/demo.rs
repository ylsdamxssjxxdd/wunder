//! Bounded, in-memory UI fixtures. No runtime, network, or file operations.
use crate::{MainWindow, TimelineRow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

const MAX_TURNS: usize = 25;
const MAX_INPUT_BYTES: usize = 16_384;

/// The demo keeps one timeline per conversation slot so switching threads and
/// sending messages behave like the native path without a runtime.
pub fn install(app: &MainWindow) {
    crate::demo_entities::install(app);
    let models: Rc<Vec<Rc<VecModel<TimelineRow>>>> = Rc::new(
        (0..9)
            .map(|index| {
                Rc::new(VecModel::from(if index == 0 {
                    demo_timeline()
                } else {
                    Vec::new()
                }))
            })
            .collect(),
    );
    app.set_timeline(ModelRc::from(models[0].clone()));
    let drafts = Rc::new(RefCell::new(vec![String::new(); 9]));
    // The conversation the preview is showing. `slot` used to be a constant 0,
    // which made `select-conversation` a no-op for the transcript.
    let active = Rc::new(RefCell::new(0usize));

    let weak = app.as_weak();
    let send_models = models.clone();
    let send_active = active.clone();
    app.on_send_message(move || {
        let Some(app) = weak.upgrade() else { return };
        let input = app.get_draft();
        let text = input.trim();
        if text.is_empty() {
            return;
        }
        if text.len() > MAX_INPUT_BYTES {
            app.set_status("演示输入过长，请缩短后重试".into());
            return;
        }
        let model = &send_models[slot(&send_active)];
        // Evict the oldest pair before appending so the preview stays bounded.
        while model.row_count() >= MAX_TURNS * 2 {
            model.remove(0);
        }
        model.push(fixture_row(
            crate::timeline::KIND_USER,
            model.row_count(),
            text,
        ));
        let reply = format!("已收到「{text}」。这是一条本地演示回复，可以继续检查条目展开与滚动效果。");
        let mut body = fixture_row(crate::timeline::KIND_BODY, model.row_count(), &reply);
        body.foldable = true;
        body.stats = demo_stats("3.6s", "51.0/s", "6.1k", "812", "");
        model.push(body);
        let group = model.row_count() as i32;
        let mut bar = fixture_row(crate::timeline::KIND_GROUP, model.row_count(), "执行工具 2 次");
        bar.open = true;
        bar.group_idx = group;
        bar.payload = group;
        model.push(bar);
        for (index, (name, target)) in [
            ("读取文件", "示例/项目/readme.md"),
            ("编辑文件", "示例/项目/main.rs"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut row = fixture_row(crate::timeline::KIND_TOOL, model.row_count(), "");
            row.tool_name = name.into();
            row.tool_icon = crate::tool_icons::workflow_icon(name).into();
            row.target = target.into();
            row.summary = "完成 · 演示结果".into();
            row.detail = "演示工具结果\n未连接本地运行时。".into();
            row.group_idx = group;
            row.group_open = true;
            row.payload = model.row_count() as i32;
            row.foldable = true;
            row.open = index == 1;
            model.push(row);
        }
        app.set_draft("".into());
        app.set_status("本地演示 · 消息未发送至后端".into());
        scroll_to_end(&app);
    });

    let weak = app.as_weak();
    let select_models = models.clone();
    let select_drafts = drafts.clone();
    let select_active = active.clone();
    app.on_select_conversation(move |index| {
        let Some(app) = weak.upgrade() else { return };
        // Switching threads stashes the draft of the one being left and shows
        // the target's own transcript, matching the native shell's isolation.
        if !(0..select_models.len() as i32).contains(&index) {
            return;
        }
        let target = index as usize;
        select_drafts.borrow_mut()[slot(&select_active)] = app.get_draft().to_string();
        *select_active.borrow_mut() = target;
        if let Some(conversation) = app.get_conversations().row_data(target) {
            app.set_heading(conversation.title);
        }
        app.set_timeline(ModelRc::from(select_models[target].clone()));
        app.set_draft(select_drafts.borrow()[target].as_str().into());
        scroll_to_end(&app);
    });

    let weak = app.as_weak();
    let new_models = models.clone();
    let new_active = active.clone();
    app.on_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        let slot = slot(&new_active);
        new_models[slot].set_vec(vec![]);
        app.set_timeline(ModelRc::from(new_models[slot].clone()));
        app.set_draft("".into());
        app.set_status("已重置当前演示任务".into());
        scroll_to_end(&app);
    });

    let counts_models = models.clone();
    let counts_active = active.clone();
    app.on_timeline_counts(move || demo_counts(&counts_models[slot(&counts_active)]));
    let answer_models = models.clone();
    let answer_active = active.clone();
    app.on_timeline_last_answer(move || {
        demo_last_answer(&answer_models[slot(&answer_active)]).into()
    });
    let weak = app.as_weak();
    let toggle_active = active.clone();
    app.on_timeline_toggle(move |payload| {
        // The row index addresses the active conversation's model directly, so
        // the window handle is only needed to keep the callback alive.
        let _ = weak.upgrade();
        let Ok(payload) = usize::try_from(payload) else {
            return;
        };
        let model = &models[slot(&toggle_active)];
        let Some(row) = model.row_data(payload) else {
            return;
        };
        match row.kind {
            crate::timeline::KIND_GROUP => {
                let open = !row.open;
                let group = row.group_idx;
                for index in 0..model.row_count() {
                    let Some(mut target) = model.row_data(index) else {
                        continue;
                    };
                    if target.group_idx != group {
                        continue;
                    }
                    if target.kind == crate::timeline::KIND_GROUP {
                        target.open = open;
                    }
                    target.group_open = open;
                    model.set_row_data(index, target);
                }
            }
            _ => {
                let mut row = row;
                row.open = !row.open;
                model.set_row_data(payload, row);
            }
        }
    });
}

fn slot(active: &Rc<RefCell<usize>>) -> usize {
    *active.borrow()
}

/// Row and answer counts of one demo timeline, matching the native callbacks.
fn demo_counts(model: &Rc<VecModel<TimelineRow>>) -> crate::TimelineCounts {
    let answers = model
        .iter()
        .filter(|row| row.kind == crate::timeline::KIND_BODY && !row.text.is_empty())
        .count();
    crate::TimelineCounts {
        rows: model.row_count() as i32,
        answers: answers as i32,
    }
}

fn demo_last_answer(model: &Rc<VecModel<TimelineRow>>) -> String {
    for index in (0..model.row_count()).rev() {
        if let Some(row) = model.row_data(index) {
            if row.kind == crate::timeline::KIND_BODY && !row.text.is_empty() {
                return row.text.to_string();
            }
        }
    }
    String::new()
}

/// The preview's metric row: same five metrics, same icon keys, same order the
/// live and reloaded rows produce.
fn demo_stats(
    duration: &str,
    speed: &str,
    context: &str,
    quota: &str,
    tools: &str,
) -> slint::ModelRc<crate::TurnStatMetric> {
    let entries = [
        ("stopwatch", duration),
        ("gauge-high", speed),
        ("layer-group", context),
        ("bolt", quota),
        ("screwdriver-wrench", tools),
    ];
    ModelRc::new(VecModel::from(
        entries
            .into_iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(key, value)| crate::TurnStatMetric {
                key: key.into(),
                value: value.into(),
            })
            .collect::<Vec<_>>(),
    ))
}

fn fixture_row(kind: i32, index: usize, text: &str) -> TimelineRow {
    TimelineRow {
        kind,
        id: format!("demo-{index}").into(),
        visible: true,
        text: text.into(),
        blocks: if text.is_empty() {
            ModelRc::default()
        } else {
            crate::message_blocks::from_text(text)
        },
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
        payload: index as i32,
        patch: ModelRc::default(),
        stats: ModelRc::default(),
    }
}

/// One completed turn so the default preview covers every entry type.
fn demo_timeline() -> Vec<TimelineRow> {
    let rows = vec![
        fixture_row(crate::timeline::KIND_USER, 0, "整理工作目录并说明结果"),
        {
            let mut body = fixture_row(
                crate::timeline::KIND_BODY,
                1,
                "已完成目录检查，并整理了可以继续使用的文件。",
            );
            // The settled turn is folded behind its divider.
            body.visible = false;
            body.foldable = true;
            body.stats = demo_stats("8.2s", "42.5/s", "8.4k", "1.2k", "2");
            body
        },
        {
            let mut divider = fixture_row(crate::timeline::KIND_DIVIDER, 2, "");
            divider.open = false;
            divider.foldable = true;
            divider.payload = 2;
            divider
        },
        fixture_row(crate::timeline::KIND_USER, 3, "检查文件内容并给出结论"),
        {
            let mut body = fixture_row(
                crate::timeline::KIND_BODY,
                4,
                "目录结构正常，文本内容可读取，未发现格式问题。",
            );
            body.foldable = true;
            body.stats = demo_stats("12.4s", "68.2k/s", "12.3k", "4.1k", "3");
            body
        },
        {
            let mut bar = fixture_row(crate::timeline::KIND_GROUP, 5, "执行工具 3 次");
            bar.open = true;
            bar.group_idx = 0;
            bar.payload = 5;
            bar
        },
        {
            let mut row = fixture_row(crate::timeline::KIND_REASON, 6, "");
            row.tool_name = "已思考".into();
            row.summary = "先读取目录清单，确认文件类型，再检查文本内容是否可用。".into();
            row.detail = row.summary.clone();
            row.group_idx = 0;
            row.group_open = true;
            row.payload = 6;
            row.foldable = true;
            row
        },
        {
            let mut row = fixture_row(crate::timeline::KIND_TOOL, 7, "");
            row.tool_name = "读取文件".into();
            row.tool_icon = "file-lines".into();
            row.target = "示例/项目/readme.md".into();
            row.summary = "完成 · 已读取 42 行".into();
            row.detail = "读取 · 完成\n示例/项目/readme.md\n演示内容".into();
            row.group_idx = 0;
            row.group_open = true;
            row.payload = 7;
            row.foldable = true;
            row
        },
        {
            let mut row = fixture_row(crate::timeline::KIND_TOOL, 8, "");
            row.tool_name = "编辑文件".into();
            row.tool_icon = "file-pen".into();
            row.target = "示例/项目/main.rs".into();
            row.summary = "完成 · 更新 +41 −41".into();
            row.group_idx = 0;
            row.group_open = true;
            row.payload = 8;
            row.foldable = true;
            row.open = true;
            row.patch = ModelRc::from(Rc::new(VecModel::from(vec![crate::PatchCard {
                path: "示例/项目/main.rs".into(),
                action: "更新".into(),
                added: "41".into(),
                deleted: "41".into(),
                lines: ModelRc::from(Rc::new(VecModel::from(vec![
                    crate::PatchLine {
                        number: "135".into(),
                        text: "fn run() {".into(),
                        kind: "context".into(),
                    },
                    crate::PatchLine {
                        number: "136".into(),
                        text: "    let previous = load();".into(),
                        kind: "delete".into(),
                    },
                    crate::PatchLine {
                        number: "136".into(),
                        text: "    let current = load_next();".into(),
                        kind: "add".into(),
                    },
                    crate::PatchLine {
                        number: "137".into(),
                        text: "}".into(),
                        kind: "context".into(),
                    },
                ]))),
            }])));
            row
        },
    ];
    rows
}

fn scroll_to_end(app: &MainWindow) {
    let weak = app.as_weak();
    // Scroll after the model's layout has caught up; weak handle avoids cycles.
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        if let Some(upgraded) = weak.upgrade() {
            upgraded.set_follow_output(true);
        }
    });
}

/// Worst case the projections allow, for the `--smoke-check` timeline
/// measurement: 50 turns (the observer's cap), 24 entries each
/// (`native_chat_turns` caps `workflow_items` at 24) and one fully expanded
/// 8x200 patch card. `unfold` marks every row of every completed turn visible,
/// which is what the user gets after unfolding all the history.
pub(crate) fn near_limit_rows(turns: usize, entries: usize, unfold: bool) -> Vec<TimelineRow> {
    let body = (0..12)
        .map(|line| format!("第 {line} 行输出内容，用于测量正文投影成本。"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut rows = Vec::new();
    for turn in 0..turns {
        let last = turn + 1 == turns;
        let visible = last || unfold;
        let mut divider = fixture_row(crate::timeline::KIND_DIVIDER, rows.len(), "");
        divider.visible = !last;
        divider.foldable = true;
        divider.payload = rows.len() as i32;
        rows.push(divider);
        let mut user = fixture_row(
            crate::timeline::KIND_USER,
            rows.len(),
            &format!("第 {turn} 轮输入"),
        );
        user.visible = visible;
        rows.push(user);
        // A batch bar plus its entries; the last entry carries the patch card so
        // the batch stays at the 24-entry ceiling instead of exceeding it.
        let base = rows.len();
        let mut bar = fixture_row(
            crate::timeline::KIND_GROUP,
            base,
            &format!("执行工具 {entries} 次"),
        );
        bar.payload = base as i32;
        bar.open = visible;
        bar.group_open = visible;
        bar.visible = visible;
        rows.push(bar);
        for slot in 0..entries {
            let index = base + 1 + slot;
            let mut row = fixture_row(crate::timeline::KIND_TOOL, index, "");
            row.tool_name = if slot + 1 == entries {
                "编辑文件".into()
            } else {
                "读取文件".into()
            };
            row.tool_icon = if slot + 1 == entries {
                "file-pen".into()
            } else {
                "file-lines".into()
            };
            row.target = format!("src/module_{slot}.rs").into();
            row.summary = "完成 · 已读取 42 行".into();
            row.detail = "读取文件 · 完成\nsrc/module_0.rs\nFixture preview".into();
            row.visible = visible;
            row.foldable = true;
            row.group_idx = base as i32;
            row.group_open = visible;
            row.open = visible && slot + 1 == entries;
            row.payload = index as i32;
            if slot + 1 == entries {
                row.patch = patch_card();
            }
            rows.push(row);
        }
        let mut answer = fixture_row(
            crate::timeline::KIND_BODY,
            rows.len(),
            &format!("{body}\n\n结论：第 {turn} 轮。"),
        );
        answer.visible = visible;
        rows.push(answer);
    }
    rows
}

/// One patch card, already bounded the way `timeline_text::patch_cards` bounds
/// it: eight files with at most 200 rendered lines each.
fn patch_card() -> ModelRc<crate::PatchCard> {
    ModelRc::from(Rc::new(VecModel::from(
        (0..8)
            .map(|file| crate::PatchCard {
                path: format!("src/module_{file}.rs").into(),
                action: "更新".into(),
                added: "200".into(),
                deleted: "100".into(),
                lines: ModelRc::from(Rc::new(VecModel::from(
                    (0..200)
                        .map(|line| crate::PatchLine {
                            number: (line + 1).to_string().into(),
                            text: "    let value = compute(input_value);".into(),
                            kind: if line % 3 == 0 { "add" } else { "context" }.into(),
                        })
                        .collect::<Vec<_>>(),
                ))),
            })
            .collect::<Vec<_>>(),
    )))
}
