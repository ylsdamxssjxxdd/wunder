//! Shared view projections for live data and the in-memory preview.
use crate::{AgentCard, MainWindow, ModelCard};
use slint::{ComponentHandle, Model};

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
    // Slint cannot slice a string, so the avatar button's fallback letter is
    // projected here while the name is still whole.
    app.set_selected_agent_initial(crate::avatar_ui::initial_letter(agent.name.as_str()).into());
    app.set_selected_agent_name(agent.name);
    app.set_selected_agent_description(agent.description);
    app.set_selected_agent_model(agent.model);
    app.set_selected_agent_system_prompt(agent.system_prompt);
    app.set_selected_agent_status(agent.status);
    app.set_selected_agent_icon_config(agent.icon_config.clone());
    // The 形象 button shows the draft's own visual, so it follows the record
    // here and the dialog after that.
    app.set_selected_agent_icon_image(agent.icon_image);
    app.set_selected_agent_tool_names(agent.tool_names);
    app.set_selected_agent_preset_questions(agent.preset_questions);
    app.set_selected_agent_approval_mode(agent.approval_mode);
    app.set_selected_agent_preview_skill(agent.preview_skill);
    app.set_selected_agent_silent(agent.silent);
    app.set_selected_agent_prefer_mother(agent.prefer_mother);
    crate::native_pages::update_agent_model_options(app);
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
    app.set_selected_model_card(model.clone());
    // The settings page edits the selected model in place, so every selection
    // projects straight into the editor drafts (secrets stay empty: an empty
    // key field means "keep the stored secret" on save).
    app.set_model_creating(false);
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
    app.set_model_video_negative_draft(model.video_negative_prompt.clone());
    app.set_selected_model_key(model.key);
    app.set_selected_model_provider(model.provider);
    app.set_selected_model_name(model.model);
    app.set_selected_model_base_url(model.base_url);
    app.set_selected_model_type(model.model_type);
    app.set_selected_model_is_default(model.is_default);
}
