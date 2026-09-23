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
        let Some(agent) = usize::try_from(index)
            .ok()
            .and_then(|i| app.get_agents().row_data(i))
        else {
            return;
        };
        app.set_selected_agent(index);
        apply_agent(&app, agent);
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
    app.set_selected_agent_name(agent.name);
    app.set_selected_agent_description(agent.description);
    app.set_selected_agent_model(agent.model);
    app.set_selected_agent_system_prompt(agent.system_prompt);
    app.set_selected_agent_status(agent.status);
    app.set_selected_agent_icon_name(agent.icon_name);
    app.set_selected_agent_icon_color(agent.icon_color);
    app.set_selected_agent_icon_glyph(agent.icon_glyph);
}

fn apply_model(app: &MainWindow, model: ModelCard) {
    app.set_selected_model_key(model.key);
    app.set_selected_model_provider(model.provider);
    app.set_selected_model_name(model.model);
    app.set_selected_model_base_url(model.base_url);
    app.set_selected_model_type(model.model_type);
    app.set_selected_model_is_default(model.is_default);
}
