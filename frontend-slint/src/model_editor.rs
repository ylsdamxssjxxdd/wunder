use crate::{MainWindow, ModelCard};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::{cell::RefCell, sync::Arc, time::Duration};
use wunder_desktop::{ModelEdit, NativeDesktop};

#[derive(Clone, PartialEq)]
struct Draft {
    card: ModelCard,
    secret: String,
}

impl Draft {
    fn read(app: &MainWindow) -> Self {
        Self {
            card: crate::entity_state::draft_model_card(app, app.get_settings_model_key().as_str()),
            secret: app.get_model_token_draft().trim().to_owned(),
        }
    }

    fn ready(&self) -> bool {
        !self.card.key.trim().is_empty()
            && !self.card.provider.trim().is_empty()
            && !self.card.model.trim().is_empty()
            && !self.card.key.starts_with("cloud/")
    }

    fn edit(&self) -> ModelEdit<'_> {
        ModelEdit {
            key: &self.card.key,
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
            image_size: &self.card.image_size,
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
    Create,
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
    if !inflight && !Draft::read(app).ready() {
        if app.get_model_creating() {
            return false;
        }
        app.set_model_save_error("请补全模型标识、提供方和模型名称".into());
        return true;
    }
    EDITOR.with(|slot| {
        if let Some(editor) = slot.borrow_mut().as_mut() {
            editor.after = Some(action);
        }
    });
    app.invoke_save_model();
    true
}

pub(crate) fn defer_selection(app: &MainWindow, key: String) -> bool {
    key != app.get_settings_model_key().as_str() && defer(app, AfterSave::Select(key))
}

pub(crate) fn defer_creation(app: &MainWindow) -> bool {
    defer(app, AfterSave::Create)
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

fn finish_action(app: &MainWindow) {
    let action = EDITOR.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .and_then(|editor| editor.after.take())
    });
    match action {
        Some(AfterSave::Select(key)) => crate::entity_state::restore_model(app, &key),
        Some(AfterSave::Create) => app.invoke_create_model(),
        Some(AfterSave::Refresh) => app.invoke_refresh_settings(),
        Some(AfterSave::Default(key)) => app.invoke_set_default_model(key.into()),
        Some(AfterSave::Delete(key)) => app.invoke_delete_model(key.into()),
        None => {}
    }
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
        if has_unsaved(&app) {
            app.set_model_save_error("".into());
            schedule(&app, Duration::from_millis(500));
        } else {
            finish_action(&app);
        }
    });
    let weak = app.as_weak();
    app.on_save_model(move || {
        let Some(app) = weak.upgrade() else { return };
        let inflight =
            EDITOR.with(|slot| slot.borrow().as_ref().is_some_and(|editor| editor.inflight));
        if inflight || app.get_model_projecting() {
            return;
        }
        if app.get_saving() || app.get_settings_loading() || app.get_agents_loading() {
            schedule(&app, Duration::from_millis(100));
            return;
        }
        let draft = Draft::read(&app);
        if !draft.ready() {
            if has_unsaved(&app) && !app.get_model_creating() {
                app.set_model_save_error("请补全模型标识、提供方和模型名称".into());
            }
            return;
        }
        if !has_unsaved(&app) && !app.get_model_creating() {
            finish_action(&app);
            return;
        }
        let creating = app.get_model_creating();
        if creating
            && app.get_settings_model_key() != draft.card.key
            && app
                .get_models()
                .iter()
                .any(|model| model.key == draft.card.key)
        {
            app.set_model_save_error("模型标识已存在，请使用其他标识".into());
            return;
        }
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
            let result = api.save_model(draft.edit());
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
                        let key = draft.card.key.to_string();
                        let projecting = app.get_model_projecting();
                        app.set_model_projecting(true);
                        if creating {
                            crate::entity_state::clear_model_draft();
                            app.set_model_creating(false);
                            app.set_settings_model_key(key.as_str().into());
                        }
                        if app.get_model_token_draft().trim() == draft.secret {
                            app.set_model_token_draft("".into());
                        }
                        let mut clean = draft;
                        clean.secret.clear();
                        EDITOR.with(|slot| {
                            if let Some(editor) = slot.borrow_mut().as_mut() {
                                editor.baseline = Some(clean);
                            }
                        });
                        crate::native_pages::apply_settings(&app, settings);
                        if let Some(index) =
                            app.get_models().iter().position(|model| model.key == key)
                        {
                            app.set_selected_model(index as i32);
                        }
                        app.set_model_projecting(projecting);
                        app.set_model_save_error("".into());
                        if changed {
                            schedule(&app, Duration::from_millis(500));
                        } else {
                            finish_action(&app);
                        }
                        app.invoke_refresh_agents();
                    }
                    Err(error) => {
                        app.set_model_save_error(format!("自动保存失败：{error}").into());
                        if changed {
                            schedule(&app, Duration::from_millis(500));
                        }
                    }
                }
            });
        });
    });
}
