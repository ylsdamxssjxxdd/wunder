//! Shared view projections for live data and the in-memory preview.
use crate::{AgentCard, MainWindow, ModelCard};
use slint::{ComponentHandle, Model};
use std::cell::RefCell;

/// The one unsaved new-model row. The catalogue keeps the row visible while
/// the user inspects other models; the editor drafts travel with it and are
/// restored when the row is selected again.
pub(crate) struct ModelDraft {
    /// Catalogue identity (the temporary "新模型" key), which stays stable
    /// even while the user edits the config key field.
    pub(crate) row_key: String,
    /// Latest editor drafts, with `key` holding the user-typed config key.
    pub(crate) card: ModelCard,
}

thread_local! {
    static MODEL_DRAFT: RefCell<Option<ModelDraft>> = const { RefCell::new(None) };
}

pub(crate) fn model_draft_row_key() -> Option<String> {
    MODEL_DRAFT.with(|slot| slot.borrow().as_ref().map(|draft| draft.row_key.clone()))
}

/// The catalogue key of the draft row the user is editing right now: the parked
/// draft when one exists, otherwise the row selected while `model-creating` is
/// set. `on_create_model` uses it to re-open the single unsaved model instead of
/// stacking a second "新模型" row.
pub(crate) fn active_model_draft_key(app: &MainWindow) -> Option<String> {
    if let Some(key) = model_draft_row_key() {
        return Some(key);
    }
    if !app.get_model_creating() {
        return None;
    }
    usize::try_from(app.get_selected_model())
        .ok()
        .and_then(|index| app.get_models().row_data(index))
        .map(|row| row.key.to_string())
}

pub(crate) fn clear_model_draft() {
    MODEL_DRAFT.with(|slot| {
        slot.borrow_mut().take();
    });
}

/// Snapshot the editor drafts of the unsaved row: write the projection back
/// into its catalogue entry so the list reflects the edits, and park the full
/// draft until the row is selected again.
pub(crate) fn stash_model_draft(app: &MainWindow) -> bool {
    if !app.get_model_creating() {
        return false;
    }
    let Some(index) = usize::try_from(app.get_selected_model()).ok() else {
        return false;
    };
    let Some(row) = app.get_models().row_data(index) else {
        return false;
    };
    let row_key = row.key.to_string();
    let card = draft_model_card(app, &row_key);
    let mut row_card = card.clone();
    row_card.key = row_key.clone().into();
    let mut rows: Vec<ModelCard> = app.get_models().iter().collect();
    if index < rows.len() {
        rows[index] = row_card;
    }
    crate::native_pages::model_catalogue().set(app, rows);
    MODEL_DRAFT.with(|slot| {
        *slot.borrow_mut() = Some(ModelDraft { row_key, card });
    });
    true
}

/// Re-apply a parked draft when its catalogue row is selected. The row keeps
/// its temporary key as the selection identity; the typed config key returns
/// to the key field.
pub(crate) fn restore_model_draft(app: &MainWindow, row_index: i32, row_key: &str) -> bool {
    let matches = MODEL_DRAFT.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|draft| draft.row_key == row_key)
    });
    if !matches {
        return false;
    }
    let card = MODEL_DRAFT.with(|slot| slot.borrow().as_ref().map(|draft| draft.card.clone()));
    let Some(card) = card else {
        return false;
    };
    apply_model_with_creation_state(app, card, true);
    app.set_settings_model_key(row_key.into());
    app.set_selected_model(row_index);
    true
}

/// Re-insert the parked draft row after a catalogue rebuild, so a settings
/// refresh cannot silently drop the unsaved model.
pub(crate) fn model_draft_row() -> Option<ModelCard> {
    MODEL_DRAFT.with(|slot| {
        slot.borrow().as_ref().map(|draft| {
            let mut row = draft.card.clone();
            row.key = draft.row_key.clone().into();
            row
        })
    })
}

/// The editor drafts projected onto a card. `fallback_key` keeps the list
/// identity when the key field is blank.
pub(crate) fn draft_model_card(app: &MainWindow, fallback_key: &str) -> ModelCard {
    let key = app.get_model_key_draft().trim().to_string();
    ModelCard {
        key: if key.is_empty() {
            fallback_key.into()
        } else {
            key.into()
        },
        provider: app.get_model_provider_draft(),
        model: app.get_model_name_draft(),
        base_url: app.get_model_base_url_draft(),
        model_type: app.get_model_type_draft(),
        is_default: false,
        is_cloud: false,
        temperature: app.get_model_temperature_draft(),
        timeout_s: app.get_model_timeout_draft(),
        max_output: app.get_model_max_output_draft(),
        thinking_token_budget: app.get_model_thinking_budget_draft(),
        max_rounds: app.get_model_max_rounds_draft(),
        max_context: app.get_model_max_context_draft(),
        history_compaction_ratio: app.get_model_history_compaction_ratio_draft(),
        reasoning_effort: app.get_model_reasoning_effort_draft(),
        simulation_speed: app.get_model_simulation_speed_draft(),
        support_vision: app.get_model_support_vision_draft(),
        support_hearing: app.get_model_support_hearing_draft(),
        stream_include_usage: app.get_model_stream_include_usage_draft(),
        tool_call_mode: app.get_model_tool_call_mode_draft(),
        tts_voice: app.get_model_tts_voice_draft(),
        tts_response_format: app.get_model_tts_format_draft(),
        tts_speed: app.get_model_tts_speed_draft(),
        tts_instructions: app.get_model_tts_instructions_draft(),
        asr_language: app.get_model_asr_language_draft(),
        asr_response_format: app.get_model_asr_format_draft(),
        asr_temperature: app.get_model_asr_temperature_draft(),
        asr_prompt: app.get_model_asr_prompt_draft(),
        image_size: app.get_model_image_size_draft(),
        image_output_format: app.get_model_image_format_draft(),
        image_steps: app.get_model_image_steps_draft(),
        image_guidance_scale: app.get_model_image_guidance_draft(),
        image_negative_prompt: app.get_model_image_negative_draft(),
        video_size: app.get_model_video_size_draft(),
        video_seconds: app.get_model_video_seconds_draft(),
        video_fps: app.get_model_video_fps_draft(),
        video_num_frames: app.get_model_video_num_frames_draft(),
        video_negative_prompt: app.get_model_video_negative_draft(),
        video_steps: app.get_model_video_steps_draft(),
        video_guidance_scale: app.get_model_video_guidance_scale_draft(),
        video_guidance_scale_2: app.get_model_video_guidance_scale_2_draft(),
        video_boundary_ratio: app.get_model_video_boundary_ratio_draft(),
        video_flow_shift: app.get_model_video_flow_shift_draft(),
        video_frame_interpolation: app.get_model_video_frame_interpolation_draft(),
        video_sync_mode: app.get_model_video_sync_mode_draft(),
        ..Default::default()
    }
}

pub fn bind_selection(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_select_agent(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() {
            return;
        }
        // Re-applying the loaded agent (post-save/refresh projection) keeps
        // the editor clean baseline; switching agents with pending edits asks
        // before discarding, mirroring the web settings panel.
        if index != app.get_selected_agent() && app.get_agent_dirty() {
            crate::agent_editor::request_leave(
                &app,
                crate::agent_editor::LeaveAction::SelectAgent(index),
            );
            return;
        }
        let Some(agent) = usize::try_from(index)
            .ok()
            .and_then(|i| app.get_agents().row_data(i))
        else {
            return;
        };
        app.set_selected_agent(index);
        apply_agent(&app, agent);
        app.set_expert_archive_page(0);
        app.set_expert_category("全部标签".into());
        app.set_expert_query("".into());
        app.set_expert_date("".into());
        app.set_expert_memories(slint::ModelRc::default());
        app.set_expert_archives(slint::ModelRc::default());
        app.set_memory_editor_open(false);
        app.invoke_refresh_expert();
    });
    let weak = app.as_weak();
    app.on_select_model(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(model) = usize::try_from(index)
            .ok()
            .and_then(|i| app.get_models().row_data(i))
        else {
            return;
        };
        if !app.get_model_projecting() && model.key == app.get_settings_model_key() {
            return;
        }
        if !app.get_model_projecting()
            && crate::model_editor::defer_selection(&app, model.key.to_string())
        {
            return;
        }
        if app.get_model_creating() {
            // Re-selecting the draft row keeps its live edits.
            if index == app.get_selected_model() {
                return;
            }
            // Park the unsaved row with its drafts instead of discarding it;
            // selecting it again restores the configuration.
            stash_model_draft(&app);
            app.set_model_creating(false);
        }
        if restore_model_draft(&app, index, model.key.as_str()) {
            return;
        }
        app.set_selected_model(index);
        apply_model(&app, model);
    });
}

pub fn restore_agent(app: &MainWindow, id: &str) {
    let index = app.get_agents().iter().position(|agent| agent.id == id);
    if app.get_agents().row_count() > 0 {
        app.invoke_select_agent(index.unwrap_or(0) as i32);
    } else {
        app.set_selected_agent(-1);
        apply_agent(app, AgentCard::default());
    }
}

/// The complete template for a new model. The catalogue card and its editor
/// draft are both projected from this value, so an earlier model selection
/// cannot leak its provider or parameters into a new card.
pub(crate) fn new_model_card(key: String) -> ModelCard {
    ModelCard {
        key: key.into(),
        provider: "openai_compatible".into(),
        model: "".into(),
        base_url: "".into(),
        model_type: "llm".into(),
        is_default: false,
        is_cloud: false,
        temperature: "0.7".into(),
        timeout_s: "120".into(),
        max_output: "8192".into(),
        thinking_token_budget: "2048".into(),
        max_rounds: "1000".into(),
        max_context: "128000".into(),
        history_compaction_ratio: "0.9".into(),
        reasoning_effort: "".into(),
        simulation_speed: "fast".into(),
        support_vision: false,
        support_hearing: false,
        stream_include_usage: true,
        tool_call_mode: "function_call".into(),
        tts_voice: "".into(),
        tts_response_format: "".into(),
        tts_speed: "1".into(),
        tts_instructions: "".into(),
        asr_language: "".into(),
        asr_response_format: "".into(),
        asr_temperature: "0".into(),
        asr_prompt: "".into(),
        image_size: "".into(),
        image_output_format: "".into(),
        image_steps: "30".into(),
        image_guidance_scale: "7.5".into(),
        image_negative_prompt: "".into(),
        video_size: "".into(),
        video_seconds: "4".into(),
        video_fps: "24".into(),
        video_num_frames: "".into(),
        video_negative_prompt: "".into(),
        video_steps: "".into(),
        video_guidance_scale: "".into(),
        video_guidance_scale_2: "".into(),
        video_boundary_ratio: "".into(),
        video_flow_shift: "".into(),
        video_frame_interpolation: false,
        video_sync_mode: false,
        ..Default::default()
    }
}

/// Project a freshly-created card without losing its creation state. Both
/// production and demo creation paths use this after inserting the card.
pub(crate) fn apply_new_model_draft(app: &MainWindow, model: ModelCard) {
    apply_model_with_creation_state(app, model, true);
}

pub fn restore_model(app: &MainWindow, key: &str) {
    let index = app.get_models().iter().position(|model| model.key == key);
    if app.get_models().row_count() > 0 {
        app.invoke_select_model(index.unwrap_or(0) as i32);
    } else {
        app.set_selected_model(-1);
        apply_model(app, ModelCard::default());
    }
}

fn apply_agent(app: &MainWindow, agent: AgentCard) {
    app.set_selected_agent_name(agent.name);
    app.set_selected_agent_description(agent.description);
    app.set_selected_agent_model(agent.model);
    app.set_selected_agent_system_prompt(agent.system_prompt);
    app.set_selected_agent_status(agent.status);
    app.set_selected_agent_icon_config(agent.icon_config.clone());
    app.set_selected_agent_tool_names(agent.tool_names);
    app.set_selected_agent_preset_questions(agent.preset_questions);
    app.set_selected_agent_approval_mode(agent.approval_mode);
    app.set_selected_agent_preview_skill(agent.preview_skill);
    app.set_selected_agent_silent(agent.silent);
    app.set_selected_agent_prefer_mother(agent.prefer_mother);
    sync_tool_selection(app);
    crate::agent_editor::refresh_agent_snapshot(app);
}

pub(crate) fn sync_tool_selection(app: &MainWindow) {
    let selected = app
        .get_selected_agent_tool_names()
        .iter()
        .map(|name| name.to_string())
        .collect::<std::collections::HashSet<_>>();
    let tools = app.get_tools();
    for index in 0..tools.row_count() {
        if let Some(mut tool) = tools.row_data(index) {
            let enabled = selected.contains(tool.name.as_str());
            if tool.enabled != enabled {
                tool.enabled = enabled;
                tools.set_row_data(index, tool);
            }
        }
    }
    crate::expert_ui::sync_tool_group_selection(app);
}

fn apply_model(app: &MainWindow, model: ModelCard) {
    apply_model_with_creation_state(app, model, false);
}

fn apply_model_with_creation_state(app: &MainWindow, model: ModelCard, creating: bool) {
    let projecting = app.get_model_projecting();
    app.set_model_projecting(true);
    app.set_selected_model_card(model.clone());
    app.set_settings_model_key(model.key.clone());
    app.set_settings_model_provider(model.provider.clone());
    app.set_settings_model_name(model.model.clone());
    app.set_settings_model_base_url(model.base_url.clone());
    app.set_settings_model_type(model.model_type.clone());
    app.set_settings_model_is_default(model.is_default);
    // The settings page edits the selected model in place, so every selection
    // projects straight into the editor drafts (secrets stay empty: an empty
    // key field means "keep the stored secret" on save).
    app.set_model_creating(creating);
    app.set_model_key_draft(model.key.clone());
    app.set_model_provider_draft(model.provider.clone());
    app.set_model_name_draft(model.model.clone());
    app.set_model_base_url_draft(model.base_url.clone());
    app.set_model_token_draft("".into());
    app.set_model_type_draft(model.model_type.clone());
    app.set_model_temperature_draft(model.temperature.clone());
    app.set_model_timeout_draft(model.timeout_s.clone());
    app.set_model_max_output_draft(model.max_output.clone());
    app.set_model_thinking_budget_draft(model.thinking_token_budget.clone());
    app.set_model_max_rounds_draft(model.max_rounds.clone());
    app.set_model_max_context_draft(model.max_context.clone());
    app.set_model_history_compaction_ratio_draft(model.history_compaction_ratio.clone());
    app.set_model_reasoning_effort_draft(model.reasoning_effort.clone());
    app.set_model_simulation_speed_draft(model.simulation_speed.clone());
    app.set_model_support_vision_draft(model.support_vision);
    app.set_model_support_hearing_draft(model.support_hearing);
    app.set_model_stream_include_usage_draft(model.stream_include_usage);
    app.set_model_tool_call_mode_draft(model.tool_call_mode.clone());
    app.set_model_tts_voice_draft(model.tts_voice.clone());
    app.set_model_tts_format_draft(model.tts_response_format.clone());
    app.set_model_tts_speed_draft(model.tts_speed.clone());
    app.set_model_tts_instructions_draft(model.tts_instructions.clone());
    app.set_model_asr_language_draft(model.asr_language.clone());
    app.set_model_asr_format_draft(model.asr_response_format.clone());
    app.set_model_asr_temperature_draft(model.asr_temperature.clone());
    app.set_model_asr_prompt_draft(model.asr_prompt.clone());
    app.set_model_image_size_draft(model.image_size.clone());
    app.set_model_image_format_draft(model.image_output_format.clone());
    app.set_model_image_steps_draft(model.image_steps.clone());
    app.set_model_image_guidance_draft(model.image_guidance_scale.clone());
    app.set_model_image_negative_draft(model.image_negative_prompt.clone());
    app.set_model_video_size_draft(model.video_size.clone());
    app.set_model_video_seconds_draft(model.video_seconds.clone());
    app.set_model_video_fps_draft(model.video_fps.clone());
    app.set_model_video_num_frames_draft(model.video_num_frames.clone());
    app.set_model_video_negative_draft(model.video_negative_prompt.clone());
    app.set_model_video_steps_draft(model.video_steps.clone());
    app.set_model_video_guidance_scale_draft(model.video_guidance_scale.clone());
    app.set_model_video_guidance_scale_2_draft(model.video_guidance_scale_2.clone());
    app.set_model_video_boundary_ratio_draft(model.video_boundary_ratio.clone());
    app.set_model_video_flow_shift_draft(model.video_flow_shift.clone());
    app.set_model_video_frame_interpolation_draft(model.video_frame_interpolation);
    app.set_model_video_sync_mode_draft(model.video_sync_mode);
    crate::model_editor::rebase(app);
    app.set_model_projecting(projecting);
    // Settings selection is intentionally separate from the composer's
    // per-thread model override. Opening a model row must not switch the
    // model shown below the composer.
}

#[cfg(test)]
mod tests {
    use super::new_model_card;

    #[test]
    fn new_model_card_uses_engine_defaults() {
        let card = new_model_card("新模型".into());
        assert_eq!(card.provider.as_str(), "openai_compatible");
        assert_eq!(card.model_type.as_str(), "llm");
        assert_eq!(card.tool_call_mode.as_str(), "function_call");
        assert_eq!(card.temperature.as_str(), "0.7");
        assert_eq!(card.timeout_s.as_str(), "120");
        assert_eq!(card.max_output.as_str(), "8192");
        assert_eq!(card.thinking_token_budget.as_str(), "2048");
        assert_eq!(card.max_rounds.as_str(), "1000");
        assert_eq!(card.max_context.as_str(), "128000");
        assert_eq!(card.history_compaction_ratio.as_str(), "0.9");
        assert_eq!(card.simulation_speed.as_str(), "fast");
        assert_eq!(card.tts_speed.as_str(), "1");
        assert_eq!(card.asr_temperature.as_str(), "0");
        assert_eq!(card.image_steps.as_str(), "30");
        assert_eq!(card.image_guidance_scale.as_str(), "7.5");
        assert_eq!(card.video_seconds.as_str(), "4");
        assert_eq!(card.video_fps.as_str(), "24");
        assert!(card.stream_include_usage);
        assert!(!card.support_vision);
        assert!(!card.support_hearing);
    }
}
