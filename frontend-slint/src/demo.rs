//! Bounded, in-memory UI fixtures. No runtime, network, or file operations.
use crate::{ChatMessage, MainWindow};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

const MAX_MESSAGES: usize = 100;
const MAX_INPUT_BYTES: usize = 16_384;

pub fn install(app: &MainWindow) {
    crate::demo_entities::install(app);
    let weak = app.as_weak();
    app.on_refresh_files(move || {
        if let Some(app) = weak.upgrade() {
            app.set_status("本地演示 · 未连接后端".into());
        }
    });
    let weak = app.as_weak();
    app.on_copy_message(move |index| {
        if let Some(app) = weak.upgrade() {
            if let Some(row) = app.get_messages().row_data(index.max(0) as usize) {
                app.invoke_copy_raw(row.text);
            }
        }
    });
    let initial: Vec<_> = app.get_messages().iter().collect();
    let models: Rc<Vec<Rc<VecModel<ChatMessage>>>> = Rc::new(
        (0..9)
            .map(|index| {
                Rc::new(VecModel::from(if index == 0 {
                    initial.clone()
                } else {
                    vec![reply("你好，可以开始新的对话。")]
                }))
            })
            .collect(),
    );
    app.set_messages(ModelRc::from(models[0].clone()));
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
        while model.row_count() + 2 > MAX_MESSAGES {
            model.remove(0);
        }
        model.push(ChatMessage {
            blocks: crate::message_blocks::from_text(text),
            text: text.into(),
            mine: true,
            time: "现在".into(),
            workflow: false,
            state: "".into(),
        });
        model.push(reply(
            "已收到。这是一条本地演示回复，可以继续检查输入、滚动与会话切换效果。",
        ));
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
        app.set_messages(ModelRc::from(select_models[slot(&app)].clone()));
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
        app.set_messages(ModelRc::from(task_models[slot(&app)].clone()));
        app.set_draft(drafts.borrow()[slot(&app)].as_str().into());
        scroll_to_end(&app);
    });

    let weak = app.as_weak();
    app.on_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        app.set_active_task(0);
        models[slot(&app)].set_vec(vec![reply("新任务已就绪，请输入消息。")]);
        app.set_messages(ModelRc::from(models[slot(&app)].clone()));
        app.set_draft("".into());
        app.set_status("已重置当前演示任务".into());
        scroll_to_end(&app);
    });
}

fn slot(app: &MainWindow) -> usize {
    (app.get_selected_conversation().clamp(0, 2) * 3 + app.get_active_task().clamp(0, 2)) as usize
}

fn reply(text: &str) -> ChatMessage {
    ChatMessage {
        blocks: crate::message_blocks::from_text(text),
        text: text.into(),
        mine: false,
        time: "现在".into(),
        workflow: false,
        state: "任务完成  ·  演示消息".into(),
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
