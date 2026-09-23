//! Entity actions in preview mode never persist configuration or credentials.
use crate::{AgentCard, MainWindow, ModelCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

pub fn install(app: &MainWindow) {
    crate::entity_state::bind_selection(app);
    crate::entity_state::restore_agent(app, "");
    crate::entity_state::restore_model(app, "");
    app.set_workspace_root("本地演示".into());
    app.set_runtime_language("zh-CN".into());
    app.on_refresh_profile(|| {});
    app.on_save_lan(|_, _| {});
    app.on_save_profile_avatar(|_, _| {});
    let weak = app.as_weak();
    app.on_create_agent(move |name| {
        let Some(app) = weak.upgrade() else { return };
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            app.set_status("名称不能为空且不能超过 80 个字符".into());
            return;
        }
        let mut agents: Vec<_> = app.get_agents().iter().collect();
        agents.insert(
            0,
            AgentCard {
                id: format!("preview-{}", agents.len()).into(),
                name: name.into(),
                status: "active".into(),
                icon_name: "spark".into(),
                icon_color: "#f97316".into(),
                icon_glyph: "✦".into(),
                icon_tone: 1,
                ..Default::default()
            },
        );
        agents.truncate(100);
        app.set_agents(ModelRc::new(VecModel::from(agents)));
        app.invoke_select_agent(0);
        app.set_agent_creator_open(false);
        app.set_status("智能体已创建 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_save_agent(move |name, description, system_prompt, model, _icon_name, _icon_color| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(index) = usize::try_from(app.get_selected_agent()) else {
            return;
        };
        let Some(mut agent) = app.get_agents().row_data(index) else {
            return;
        };
        if name.trim().is_empty() {
            return;
        }
        agent.name = name;
        agent.description = description;
        agent.system_prompt = system_prompt;
        agent.model = model;
        app.get_agents().set_row_data(index, agent);
        app.invoke_select_agent(index as i32);
        app.set_status("配置已保存 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_save_runtime(move |workspace, language| {
        if let Some(app) = weak.upgrade() {
            app.set_workspace_root(workspace);
            app.set_runtime_language(language);
            app.set_status("设置已保存 · 仅本次演示有效".into());
        }
    });
    let weak = app.as_weak();
    app.on_save_model(move |key, provider, model, base_url, _token, kind| {
        let Some(app) = weak.upgrade() else { return };
        if key.trim().is_empty()
            || model.trim().is_empty()
            || provider.trim().is_empty()
            || !matches!(
                kind.as_str(),
                "llm" | "embedding" | "asr" | "tts" | "image" | "video"
            )
        {
            app.set_status("请填写模型配置，并选择有效的模型类型".into());
            return;
        }
        let mut models: Vec<_> = app.get_models().iter().collect();
        let index = models.iter().position(|entry| entry.key == key);
        let entry = ModelCard {
            key: key.clone(),
            provider,
            model,
            base_url,
            model_type: kind,
            is_default: index.is_some_and(|i| models[i].is_default),
        };
        if let Some(index) = index {
            models[index] = entry;
        } else {
            models.insert(0, entry);
        }
        models.truncate(200);
        app.set_models(ModelRc::new(VecModel::from(models)));
        crate::entity_state::restore_model(&app, &key);
        app.set_model_editor_open(false);
        app.set_model_token_draft("".into());
        app.set_status("模型已保存 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_set_default_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        let mut models: Vec<_> = app.get_models().iter().collect();
        let Some(selected) = models.iter().find(|entry| entry.key == key) else {
            return;
        };
        let kind = selected.model_type.clone();
        for model in &mut models {
            if model.model_type == kind {
                model.is_default = model.key == key;
            }
        }
        app.set_models(ModelRc::new(VecModel::from(models)));
        crate::entity_state::restore_model(&app, &key);
        app.set_status("默认模型已更新 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_refresh_agents(move || {
        if let Some(app) = weak.upgrade() {
            app.set_status("本地演示 · 未连接后端".into());
        }
    });
    let weak = app.as_weak();
    app.on_refresh_tools(move || {
        if let Some(app) = weak.upgrade() {
            app.set_status("本地演示 · 未连接后端".into());
        }
    });
    let weak = app.as_weak();
    app.on_refresh_settings(move || {
        if let Some(app) = weak.upgrade() {
            app.set_status("本地演示 · 未连接后端".into());
        }
    });
}
