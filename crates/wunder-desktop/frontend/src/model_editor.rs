use crate::{MainWindow, ModelCard};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{cell::RefCell, sync::Arc, time::Duration};
use wunder_desktop::{ModelEdit, NativeDesktop};

#[derive(Clone, PartialEq)]
struct Draft {
    card: ModelCard,
    secret: String,
    /// Stored key of the row being edited. The key field is an identity: it is
    /// only sent as a rename when it differs from this value.
    original_key: String,
}

impl Draft {
    fn read(app: &MainWindow) -> Self {
        Self {
            card: crate::entity_state::draft_model_card(app, app.get_settings_model_key().as_str()),
            secret: app.get_model_token_draft().trim().to_owned(),
            original_key: app.get_settings_model_key().to_string(),
        }
    }

    fn ready(&self) -> bool {
        !self.card.key.trim().is_empty()
            && !self.card.provider.trim().is_empty()
            && !self.card.model.trim().is_empty()
            && !self.card.key.starts_with("cloud/")
    }

    fn edit(&self, renaming: bool) -> ModelEdit<'_> {
        let key = if renaming || self.original_key.is_empty() {
            self.card.key.as_str()
        } else {
            // Non-rename saves always address the stored entry, so a half-typed
            // key cannot move the config out from under the row.
            self.original_key.as_str()
        };
        ModelEdit {
            key,
            original_key: self.original_key.as_str(),
            provider: &self.card.provider,
            model: &self.card.model,
            base_url: &self.card.base_url,
            api_key: &self.secret,
            model_type: &self.card.model_type,
            temperature: &self.card.temperature,
            timeout_s: &self.card.timeout_s,
            max_output: &self.card.max_output,
            thinking_token_budget: &self.card.thinking_token_budget,
            max_rounds: &self.card.max_rounds,
            max_context: &self.card.max_context,
            history_compaction_ratio: &self.card.history_compaction_ratio,
            reasoning_effort: &self.card.reasoning_effort,
            simulation_speed: &self.card.simulation_speed,
            support_vision: self.card.support_vision,
            support_hearing: self.card.support_hearing,
            stream_include_usage: self.card.stream_include_usage,
            tool_call_mode: &self.card.tool_call_mode,
            tts_voice: &self.card.tts_voice,
            tts_response_format: &self.card.tts_response_format,
            tts_speed: &self.card.tts_speed,
            tts_instructions: &self.card.tts_instructions,
            asr_language: &self.card.asr_language,
            asr_response_format: &self.card.asr_response_format,
            asr_temperature: &self.card.asr_temperature,
            asr_prompt: &self.card.asr_prompt,
            image_default_size: &self.card.image_default_size,
            image_max_size: &self.card.image_max_size,
            image_output_format: &self.card.image_output_format,
            image_steps: &self.card.image_steps,
            image_guidance_scale: &self.card.image_guidance_scale,
            image_negative_prompt: &self.card.image_negative_prompt,
            video_size: &self.card.video_size,
            video_seconds: &self.card.video_seconds,
            video_fps: &self.card.video_fps,
            video_num_frames: &self.card.video_num_frames,
            video_negative_prompt: &self.card.video_negative_prompt,
            video_steps: &self.card.video_steps,
            video_guidance_scale: &self.card.video_guidance_scale,
            video_guidance_scale_2: &self.card.video_guidance_scale_2,
            video_boundary_ratio: &self.card.video_boundary_ratio,
            video_flow_shift: &self.card.video_flow_shift,
            video_frame_interpolation: self.card.video_frame_interpolation,
            video_sync_mode: self.card.video_sync_mode,
        }
    }
}

#[derive(Clone)]
enum AfterSave {
    Select(String),
    Refresh,
    Default(String),
    Delete(String),
}

#[derive(Default)]
struct Editor {
    timer: Timer,
    baseline: Option<Draft>,
    inflight: bool,
    after: Option<AfterSave>,
}

thread_local! {
    static EDITOR: RefCell<Option<Editor>> = const { RefCell::new(None) };
}

pub(crate) fn rebase(app: &MainWindow) {
    let draft = Draft::read(app);
    EDITOR.with(|slot| {
        if let Some(editor) = slot.borrow_mut().as_mut() {
            editor.baseline = Some(draft);
            editor.timer.stop();
        }
    });
    app.set_model_save_error("".into());
}

pub(crate) fn has_unsaved(app: &MainWindow) -> bool {
    let draft = Draft::read(app);
    EDITOR.with(|slot| {
        slot.borrow().as_ref().is_some_and(|editor| {
            editor.inflight
                || editor
                    .baseline
                    .as_ref()
                    .is_some_and(|clean| clean != &draft)
        })
    })
}

/// 保存排队：正在保存或设置仍在加载时，把这次显式保存顺延一次。
fn schedule(app: &MainWindow, delay: Duration) {
    let weak = app.as_weak();
    EDITOR.with(|slot| {
        if let Some(editor) = slot.borrow().as_ref() {
            editor.timer.start(TimerMode::SingleShot, delay, move || {
                if let Some(app) = weak.upgrade() {
                    app.invoke_save_model();
                }
            });
        }
    });
}

fn defer(app: &MainWindow, action: AfterSave) -> bool {
    if !has_unsaved(app) {
        return false;
    }
    let inflight = EDITOR.with(|slot| slot.borrow().as_ref().is_some_and(|editor| editor.inflight));
    // 删除针对的是已落库的配置：草稿里没保存的改动（哪怕模型名称还空着）
    // 没有保留价值，直接丢弃再删，不该反过来拦下删除。保存中的请求仍等它
    // 落地，避免删除与写入竞态。
    if !inflight && matches!(action, AfterSave::Delete(_)) {
        rebase(app);
        return false;
    }
    if inflight {
        // 保存已经发出，落地后接着执行用户要求的动作。
        EDITOR.with(|slot| {
            if let Some(editor) = slot.borrow_mut().as_mut() {
                editor.after = Some(action);
            }
        });
        return true;
    }
    // 落库只认「保存」按钮（以及回车提交配置名称）：切换/新建/刷新/设默认
    // 遇到未保存的改动先提示保存，不再隐式替用户保存。
    let message = app
        .global::<crate::I18n>()
        .get_st_model_unsaved()
        .to_string();
    app.set_model_save_error(message.as_str().into());
    crate::native_pages::show_toast(app, message, 2);
    true
}

pub(crate) fn defer_selection(app: &MainWindow, key: String) -> bool {
    key != app.get_settings_model_key().as_str() && defer(app, AfterSave::Select(key))
}

pub(crate) fn defer_refresh(app: &MainWindow) -> bool {
    defer(app, AfterSave::Refresh)
}

pub(crate) fn defer_default(app: &MainWindow, key: String) -> bool {
    defer(app, AfterSave::Default(key))
}

pub(crate) fn defer_delete(app: &MainWindow, key: String) -> bool {
    defer(app, AfterSave::Delete(key))
}

/// 保存成功的提示文案。首次配置的真实模型会自动接管默认位（见
/// `NativeDesktop::save_model`），那时要顺带说明它已经是默认模型。
fn saved_message(app: &MainWindow, key: &str) -> String {
    let i18n = app.global::<crate::I18n>();
    let is_default = app
        .get_models()
        .iter()
        .any(|model| model.key == key && model.is_default);
    if is_default {
        i18n.get_st_model_saved_default().to_string()
    } else {
        i18n.get_st_model_saved().to_string()
    }
}

fn finish_action(app: &MainWindow) {
    let action = EDITOR.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .and_then(|editor| editor.after.take())
    });
    match action {
        Some(AfterSave::Select(key)) => crate::entity_state::restore_model(app, &key),
        Some(AfterSave::Refresh) => app.invoke_refresh_settings(),
        Some(AfterSave::Default(key)) => app.invoke_set_default_model(key.into()),
        Some(AfterSave::Delete(key)) => app.invoke_delete_model(key.into()),
        None => {}
    }
}

/// 「添加模型」立即生成一条完整配置并落盘：唯一标识 + 占位模型名 + 默认参数。
/// 之后它就是一条真实配置，编辑、保存、删除都按普通模型走，不再有「未保存的
/// 草稿行」这种只存在于前端的半成品。
/// 新配置的标识：`新模型`、`新模型1`、`新模型2`……取第一个没被占用的。
pub(crate) fn next_model_key<'a>(taken: impl Iterator<Item = &'a str>) -> String {
    let taken = taken.collect::<Vec<_>>();
    let mut key = "新模型".to_string();
    let mut suffix = 1usize;
    while taken.contains(&key.as_str()) {
        key = format!("新模型{suffix}");
        suffix += 1;
    }
    key
}

pub(crate) fn create_model_now(app: &MainWindow, api: &Arc<NativeDesktop>) {
    if app.get_saving() || app.get_settings_loading() {
        return;
    }
    let key = {
        let keys = app
            .get_models()
            .iter()
            .map(|entry| entry.key.to_string())
            .collect::<Vec<_>>();
        next_model_key(keys.iter().map(String::as_str))
    };
    let mut card = crate::entity_state::new_model_card(key.clone());
    // 后端要求模型名非空；占位值让配置一落盘就合法，用户再改成真实模型名。
    card.model = "new-model".into();
    // 当前表单还有未保存的改动时不静默丢弃：先提示保存。
    if has_unsaved(app) {
        let message = app
            .global::<crate::I18n>()
            .get_st_model_unsaved()
            .to_string();
        app.set_model_save_error(message.as_str().into());
        crate::native_pages::show_toast(app, message, 2);
        return;
    }
    let draft = Draft {
        card,
        secret: String::new(),
        original_key: String::new(),
    };
    app.set_saving(true);
    let weak = app.as_weak();
    let api = api.clone();
    let created_key = key;
    crate::native_pages::run_background(move || {
        let result = api.save_model(draft.edit(false));
        let _ = weak.upgrade_in_event_loop(move |app| {
            app.set_saving(false);
            match result {
                Ok(settings) => {
                    crate::native_pages::apply_settings(&app, settings);
                    crate::entity_state::restore_model(&app, &created_key);
                    let message = app
                        .global::<crate::I18n>()
                        .get_st_model_added()
                        .to_string();
                    app.set_status(message.as_str().into());
                    crate::native_pages::show_toast(&app, message, 1);
                    app.invoke_refresh_agents();
                }
                Err(error) => {
                    crate::native_pages::show_error(&app, format!("无法添加模型：{}", error))
                }
            }
        });
    });
}

/// 保存当前模型草稿。落库只有这一条路径：表单上的「保存」、回车提交配置名称，
/// 以及保存排队后的重试；编辑器不再做防抖自动保存。
fn run_save(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let inflight =
        EDITOR.with(|slot| slot.borrow().as_ref().is_some_and(|editor| editor.inflight));
    if inflight || app.get_model_projecting() {
        return;
    }
    if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
        schedule(app, Duration::from_millis(100));
        return;
    }
    let draft = Draft::read(app);
    if !draft.ready() {
        // 配置一落库就合法；用户清空了必填项时，显式保存要说清缺什么。
        if has_unsaved(app) {
            app.set_model_save_error("请补全模型标识、提供方和模型名称".into());
        }
        return;
    }
    if !has_unsaved(app) {
        // 点「保存」但没有改动：也要有回应，避免按钮像没反应。
        let message = app
            .global::<crate::I18n>()
            .get_st_model_unchanged()
            .to_string();
        app.set_status(message.as_str().into());
        crate::native_pages::show_toast(app, message, 1);
        finish_action(app);
        return;
    }
    // 保存是显式动作：改了标识就按改名提交（后端负责迁移引用与默认位）。
    let renaming = !draft.original_key.is_empty() && draft.card.key.trim() != draft.original_key;
    EDITOR.with(|slot| {
        if let Some(editor) = slot.borrow_mut().as_mut() {
            editor.timer.stop();
            editor.inflight = true;
        }
    });
    app.set_saving(true);
    let weak = app.as_weak();
    let api = api.clone();
    crate::native_pages::run_background(move || {
        let result = api.save_model(draft.edit(renaming));
        let _ = weak.upgrade_in_event_loop(move |app| {
            let changed = Draft::read(&app) != draft;
            EDITOR.with(|slot| {
                if let Some(editor) = slot.borrow_mut().as_mut() {
                    editor.inflight = false;
                }
            });
            app.set_saving(false);
            match result {
                Ok(settings) => {
                    // The key actually stored: a rename lands on the typed
                    // key, every other save addresses the existing entry.
                    let key = if renaming || draft.original_key.is_empty() {
                        draft.card.key.to_string()
                    } else {
                        draft.original_key.clone()
                    };
                    let projecting = app.get_model_projecting();
                    app.set_model_projecting(true);
                    if app.get_settings_model_key() != key {
                        app.set_settings_model_key(key.as_str().into());
                    }
                    if app.get_model_token_draft().trim() == draft.secret {
                        app.set_model_token_draft("".into());
                    }
                    let mut clean = draft.clone();
                    clean.secret.clear();
                    clean.card.key = key.as_str().into();
                    clean.original_key = key.clone();
                    EDITOR.with(|slot| {
                        if let Some(editor) = slot.borrow_mut().as_mut() {
                            editor.baseline = Some(clean);
                        }
                    });
                    // The composer follows the renamed model. The key it
                    // showed has to be captured before `apply_settings`
                    // re-projects the catalogue: by then a pin on the old
                    // key has already been dropped to the fallback, and the
                    // composer would keep labelling the thread with a
                    // different model than the one it was pinned to.
                    let composer_key = app.get_selected_model_key().to_string();
                    crate::native_pages::apply_settings(&app, settings);
                    if let Some(index) =
                        app.get_models().iter().position(|model| model.key == key)
                    {
                        app.set_selected_model(index as i32);
                    }
                    if renaming
                        && (composer_key.is_empty()
                            || composer_key == draft.original_key.as_str())
                    {
                        crate::native_pages::select_model_key(&app, &key);
                    }
                    app.set_model_projecting(projecting);
                    app.set_model_save_error("".into());
                    // 每次保存都要回答「点下去有没有生效」。
                    crate::native_pages::show_toast(&app, saved_message(&app, &key), 1);
                    // 保存期间的新输入保持未保存状态，等用户再点保存；
                    // 不再自动落库。
                    if !changed {
                        finish_action(&app);
                    }
                    app.invoke_refresh_agents();
                }
                Err(error) => {
                    app.set_model_save_error(format!("保存失败：{error}").into());
                }
            }
        });
    });
}

pub(crate) fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    EDITOR.with(|slot| *slot.borrow_mut() = Some(Editor::default()));
    rebase(app);
    let weak = app.as_weak();
    app.on_model_edited(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_model_projecting() || app.get_settings_model_key().starts_with("cloud/") {
            return;
        }
        // 模型配置只在显式保存时落库（防抖自动保存已移除）；输入过程中
        // 只清掉上一次的错误提示。
        app.set_model_save_error("".into());
    });
    let save_api = api.clone();
    let weak = app.as_weak();
    app.on_save_model(move || {
        let Some(app) = weak.upgrade() else { return };
        run_save(&app, &save_api);
    });
    let weak = app.as_weak();
    app.on_commit_model_key(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_model_projecting() || app.get_settings_model_key().starts_with("cloud/") {
            return;
        }
        let typed = app.get_model_key_draft();
        if typed.trim().is_empty() || typed.trim() == app.get_settings_model_key().as_str() {
            return;
        }
        app.invoke_save_model();
    });
}

#[cfg(test)]
mod tests {
    use super::next_model_key;

    #[test]
    fn new_model_keys_take_the_first_free_suffix() {
        let key = |taken: &[&str]| next_model_key(taken.iter().copied());
        assert_eq!(key(&[]), "新模型");
        assert_eq!(key(&["other-model"]), "新模型");
        assert_eq!(key(&["新模型"]), "新模型1");
        assert_eq!(key(&["新模型1", "新模型"]), "新模型2");
        assert_eq!(key(&["新模型", "新模型2"]), "新模型1");
    }
}
