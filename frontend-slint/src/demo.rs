//! Bounded, in-memory UI fixtures. No runtime, network, or file operations.
use crate::{ChatMessage, ChatTurn, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

const MAX_MESSAGES: usize = 100;
const MAX_INPUT_BYTES: usize = 16_384;

pub fn install(app: &MainWindow) {
    crate::demo_entities::install(app);
    // Keep the default preview representative of a completed agent turn so
    // visual smoke captures cover reasoning and expandable tool entries.
    app.set_turns(ModelRc::from(Rc::new(VecModel::from(vec![demo_turn()]))));
    let weak = app.as_weak();
    app.on_refresh_files(move || {
        if let Some(app) = weak.upgrade() {
            app.set_status("本地演示 · 未连接后端".into());
        }
    });
    let weak = app.as_weak();
    app.on_copy_message(move |index| {
        if let Some(app) = weak.upgrade() {
            if let Some(row) = app.get_turns().row_data(index.max(0) as usize) {
                app.invoke_copy_raw(row.assistant.text);
            }
        }
    });
    let initial: Vec<_> = app.get_turns().iter().collect();
    let models: Rc<Vec<Rc<VecModel<ChatTurn>>>> = Rc::new(
        (0..9)
            .map(|index| {
                Rc::new(VecModel::from(if index == 0 {
                    initial.clone()
                } else {
                    vec![]
                }))
            })
            .collect(),
    );
    app.set_turns(ModelRc::from(models[0].clone()));
    let drafts = Rc::new(RefCell::new(vec![String::new(); 9]));

    let weak = app.as_weak();
    let send_models = models.clone();
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
        let index = slot(&app);
        let model = &send_models[index];
        // Evict the oldest pair before appending so the preview stays bounded.
        while model.row_count() >= MAX_MESSAGES / 2 {
            model.remove(0);
        }
        let user = ChatMessage {
            blocks: crate::message_blocks::from_text(text),
            text: text.into(),
            mine: true,
            time: "现在".into(),
            workflow: false,
            workflow_detail: "".into(),
            workflow_items: ModelRc::default(),
            reasoning: "".into(),
            reasoning_streaming: false,
            state: "".into(),
            stats_status: "".into(),
            stats_duration: "".into(),
            stats_speed: "".into(),
            stats_context: "".into(),
            stats_quota: "".into(),
            stats_tools: "".into(),
            stats_credits: "".into(),
            avatar_glyph: "".into(),
            avatar_tone: 0,
        };
        model.push(ChatTurn {
            user,
            assistant: reply(
                "已收到。这是一条本地演示回复，可以继续检查输入、滚动与会话切换效果。",
            ),
            ..Default::default()
        });
        app.set_draft("".into());
        app.set_status("本地演示 · 消息未发送至后端".into());
        scroll_to_end(&app);
    });

    let weak = app.as_weak();
    let select_models = models.clone();
    let select_drafts = drafts.clone();
    app.on_select_conversation(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if !(0..3).contains(&index) {
            return;
        }
        select_drafts.borrow_mut()[slot(&app)] = app.get_draft().to_string();
        app.set_selected_conversation(index);
        if let Some(conversation) = app.get_conversations().row_data(index as usize) {
            app.set_heading(conversation.title);
        }
        app.set_active_task(0);
        app.set_turns(ModelRc::from(select_models[slot(&app)].clone()));
        app.set_draft(select_drafts.borrow()[slot(&app)].as_str().into());
        scroll_to_end(&app);
    });

    let weak = app.as_weak();
    let task_models = models.clone();
    app.on_select_task(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if !(0..3).contains(&index) {
            return;
        }
        drafts.borrow_mut()[slot(&app)] = app.get_draft().to_string();
        app.set_active_task(index);
        app.set_turns(ModelRc::from(task_models[slot(&app)].clone()));
        app.set_draft(drafts.borrow()[slot(&app)].as_str().into());
        scroll_to_end(&app);
    });

    let weak = app.as_weak();
    app.on_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_active_task(0);
        models[slot(&app)].set_vec(vec![]);
        app.set_turns(ModelRc::from(models[slot(&app)].clone()));
        app.set_draft("".into());
        app.set_status("已重置当前演示任务".into());
        scroll_to_end(&app);
    });
}

fn slot(app: &MainWindow) -> usize {
    (app.get_selected_conversation().clamp(0, 2) * 3 + app.get_active_task().clamp(0, 2)) as usize
}

fn reply(text: &str) -> ChatMessage {
    let workflow_detail = "读取工作目录\n已读取并整理文件列表";
    ChatMessage {
        blocks: crate::message_blocks::from_text(text),
        text: text.into(),
        mine: false,
        time: "现在".into(),
        workflow: true,
        workflow_detail: workflow_detail.into(),
        workflow_items: ModelRc::from(Rc::new(VecModel::from(vec![crate::ToolWorkflowEntry {
            id: "demo-tool".into(), title: "读取工作目录".into(), preview: "已读取并整理文件列表".into(), detail: workflow_detail.into(), state: "completed".into()
        }]))),
        reasoning: "先检查工作目录中的文件，再整理结果并生成回复。".into(),
        reasoning_streaming: false,
        state: "".into(),
        stats_status: "任务完成".into(),
        stats_duration: "1.2s".into(),
        stats_speed: "42.0/s".into(),
        stats_context: "2.4k".into(),
        stats_quota: "320".into(),
        stats_tools: "2".into(),
        stats_credits: "1".into(),
        avatar_glyph: "✦".into(),
        avatar_tone: 1,
    }
}

fn demo_turn() -> ChatTurn {
    ChatTurn {
        root_id: "demo-workflow".into(),
        user: ChatMessage {
            text: "整理工作目录并说明结果".into(),
            blocks: crate::message_blocks::from_text("整理工作目录并说明结果"),
            mine: true,
            time: "现在".into(),
            ..Default::default()
        },
        assistant: ChatMessage {
            text: "已完成目录检查，并整理了可以继续使用的文件。".into(),
            blocks: crate::message_blocks::from_text("已完成目录检查，并整理了可以继续使用的文件。"),
            mine: false,
            time: "现在".into(),
            workflow: true,
            workflow_detail: "读取工作目录\n已读取并整理文件列表\n\n检查文件内容\n已验证文本内容可读取".into(),
            workflow_items: ModelRc::from(Rc::new(VecModel::from(vec![
                crate::ToolWorkflowEntry { id: "read-dir".into(), title: "读取工作目录".into(), preview: "已读取并整理文件列表".into(), detail: "执行完成\n找到 2 个可用文件，目录结构正常。".into(), state: "completed".into() },
                crate::ToolWorkflowEntry { id: "inspect-file".into(), title: "检查文件内容".into(), preview: "已验证文本内容可读取".into(), detail: "执行完成\n文件内容已读取，未发现格式错误。".into(), state: "completed".into() }
            ]))),
            reasoning: "先读取目录清单，确认文件类型，再检查文本内容是否可用，最后汇总结果。".into(),
            reasoning_streaming: false,
            stats_status: "任务完成".into(),
            stats_duration: "1.2s".into(),
            stats_speed: "42.0/s".into(),
            stats_context: "2.4k".into(),
            stats_quota: "320".into(),
            stats_tools: "2".into(),
            stats_credits: "1".into(),
            avatar_glyph: "✦".into(),
            avatar_tone: 1,
            ..Default::default()
        },
    }
}

fn scroll_to_end(app: &MainWindow) {
    let weak = app.as_weak();
    // Scroll after the model's layout has caught up; weak handle avoids cycles.
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        if let Some(app) = weak.upgrade() {
            app.set_scroll_revision(app.get_scroll_revision().wrapping_add(1));
        }
    });
}
