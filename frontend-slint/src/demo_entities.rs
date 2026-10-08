//! Entity actions in preview mode never persist configuration or credentials.
use crate::{MainWindow, ModelCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

thread_local! {
    /// The preview's unfiltered model catalogue, mirroring the native shell: the
    /// composer popover renders the filtered view, so a query must not destroy
    /// the list it filters.
    static DEMO_MODELS: Rc<RefCell<Vec<ModelCard>>> = Rc::new(RefCell::new(Vec::new()));
}

/// Models the preview starts with. Names stay generic on purpose: fixtures must
/// not carry a real provider, endpoint or credential.
fn seed_models() -> Vec<ModelCard> {
    vec![
        ModelCard {
            key: "demo-local".into(),
            provider: "本地".into(),
            model: "演示模型·本地".into(),
            base_url: "".into(),
            model_type: "llm".into(),
            is_default: true,
            max_context: "32768".into(),
            ..Default::default()
        },
        ModelCard {
            key: "demo-remote".into(),
            provider: "远端".into(),
            model: "演示模型·远端".into(),
            base_url: "".into(),
            model_type: "llm".into(),
            is_default: false,
            max_context: "131072".into(),
            ..Default::default()
        },
        ModelCard {
            key: "demo-voice".into(),
            provider: "本地".into(),
            model: "演示语音·转写".into(),
            base_url: "".into(),
            model_type: "asr".into(),
            is_default: false,
            ..Default::default()
        },
    ]
}

/// Shows the catalogue unfiltered and mirrors its size for the popover's
/// "no match" versus "none added" distinction.
fn project_models(app: &MainWindow, query: &str) {
    let needle = query.trim().to_lowercase();
    DEMO_MODELS.with(|models| {
        let models = models.borrow();
        app.set_model_catalogue_count(models.len() as i32);
        let rows: Vec<ModelCard> = if needle.is_empty() {
            models.clone()
        } else {
            models
                .iter()
                .filter(|model| {
                    [&model.model, &model.provider, &model.model_type, &model.key]
                        .iter()
                        .any(|field| field.to_lowercase().contains(&needle))
                })
                .cloned()
                .collect()
        };
        app.set_models(ModelRc::new(VecModel::from(rows)));
    });
}

/// Replaces the preview catalogue; the model editor and the delete action both
/// go through here so search can never see a stale list.
fn store_models(app: &MainWindow, models: Vec<ModelCard>) {
    DEMO_MODELS.with(|slot| *slot.borrow_mut() = models);
    project_models(app, "");
}

/// The personal build ships exactly one built-in agent. The preview seeds the
/// same shape so the settings category and the shell contract both have
/// something real to render; nothing here is persisted.
fn seed_agent() -> crate::AgentCard {
    crate::AgentCard {
        id: "demo-default".into(),
        name: "内置智能体".into(),
        description: "本地演示用内置智能体".into(),
        model: "演示模型·本地".into(),
        system_prompt: "演示用系统提示词：线程首次确定后保持冻结。".into(),
        status: "ready".into(),
        approval_mode: "full_auto".into(),
        ..Default::default()
    }
}

pub fn install(app: &MainWindow) {
    crate::entity_state::bind_selection(app);
    app.set_agents(ModelRc::new(VecModel::from(vec![seed_agent()])));
    app.set_selected_agent(0);
    crate::entity_state::restore_agent(app, "");
    store_models(app, seed_models());
    crate::entity_state::restore_model(app, "demo-local");
    app.set_workspace_root("本地演示".into());
    app.set_runtime_language("zh-CN".into());
    app.on_refresh_profile(|| {});
    app.on_save_lan(|_, _| {});
    app.on_save_profile_avatar(|_, _| {});
    // The preview mirrors the native shell's search behaviour, otherwise the
    // composer's model popover and the settings navigation would look broken
    // in the screenshots this path exists to produce.
    let weak = app.as_weak();
    app.on_search_models(move |query| {
        let Some(app) = weak.upgrade() else { return };
        project_models(&app, &query);
    });
    let weak = app.as_weak();
    app.on_filter_settings_categories(move |query| {
        let Some(app) = weak.upgrade() else { return };
        app.set_settings_category_flags(ModelRc::new(VecModel::from(
            crate::settings_search::category_flags(&query),
        )));
    });
    let weak = app.as_weak();
    app.on_save_agent(
        move |name,
              description,
              system_prompt,
              model,
              _icon_config,
              tool_names,
              preset_questions,
              _sandbox_container_id,
              approval_mode,
              preview_skill,
              silent,
              prefer_mother| {
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
            agent.tool_names = tool_names;
            agent.tool_count = agent.tool_names.row_count() as i32;
            agent.preset_questions = preset_questions;
            agent.preset_question_count = agent.preset_questions.row_count() as i32;
            agent.approval_mode = approval_mode;
            agent.preview_skill = preview_skill;
            agent.silent = silent;
            agent.prefer_mother = prefer_mother;
            app.get_agents().set_row_data(index, agent);
            crate::agent_editor::refresh_agent_snapshot(&app);
            app.invoke_select_agent(index as i32);
            app.set_status("配置已保存 · 仅本次演示有效".into());
        },
    );
    let weak = app.as_weak();
    app.on_toggle_agent_tool(move |name| {
        if let Some(app) = weak.upgrade() {
            let mut names = app
                .get_selected_agent_tool_names()
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            if let Some(index) = names.iter().position(|value| value == name.as_str()) {
                names.remove(index);
            } else {
                names.push(name.to_string());
            }
            app.set_selected_agent_tool_names(ModelRc::new(VecModel::from(
                names
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<slint::SharedString>>(),
            )));
            crate::entity_state::sync_tool_selection(&app);
        }
    });
    let weak = app.as_weak();
    app.on_add_agent_question(move || {
        if let Some(app) = weak.upgrade() {
            let draft = app.get_agent_question_draft().trim().to_string();
            if !draft.is_empty() {
                let mut values = app
                    .get_selected_agent_preset_questions()
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>();
                values.push(draft);
                app.set_selected_agent_preset_questions(ModelRc::new(VecModel::from(
                    values
                        .into_iter()
                        .map(Into::into)
                        .collect::<Vec<slint::SharedString>>(),
                )));
                app.set_agent_question_draft("".into());
            }
        }
    });
    let weak = app.as_weak();
    app.on_remove_agent_question(move |index| {
        if let Some(app) = weak.upgrade() {
            let mut values = app
                .get_selected_agent_preset_questions()
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>();
            if let Ok(index) = usize::try_from(index) {
                if index < values.len() {
                    values.remove(index);
                    app.set_selected_agent_preset_questions(ModelRc::new(VecModel::from(
                        values
                            .into_iter()
                            .map(Into::into)
                            .collect::<Vec<slint::SharedString>>(),
                    )));
                }
            }
        }
    });
    let weak = app.as_weak();
    app.on_save_runtime(move |workspace, language, _python, _git, _rg| {
        if let Some(app) = weak.upgrade() {
            app.set_workspace_root(workspace);
            app.set_runtime_language(language);
            app.set_status("设置已保存 · 仅本次演示有效".into());
        }
    });
    let weak = app.as_weak();
    app.on_import_supplement(move || {
        if let Some(app) = weak.upgrade() {
            app.set_supplement_status("演示模式不支持导入补充包".into());
        }
    });
    app.on_pick_python_path(|| {});
    app.on_pick_git_path(|| {});
    app.on_pick_rg_path(|| {});
    let weak = app.as_weak();
    app.on_save_model(move || {
        let Some(app) = weak.upgrade() else { return };
        let key = app.get_model_key_draft().trim().to_string();
        let provider = app.get_model_provider_draft().trim().to_string();
        let model = app.get_model_name_draft().trim().to_string();
        let base_url = app.get_model_base_url_draft().trim().to_string();
        let kind = app.get_model_type_draft().trim().to_string();
        if key.is_empty()
            || model.is_empty()
            || provider.is_empty()
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
            key: key.clone().into(),
            provider: provider.into(),
            model: model.into(),
            base_url: base_url.into(),
            model_type: kind.into(),
            is_default: index.is_some_and(|i| models[i].is_default),
            ..Default::default()
        };
        if let Some(index) = index {
            models[index] = entry;
        } else {
            models.insert(0, entry);
        }
        models.truncate(200);
        store_models(&app, models);
        crate::entity_state::restore_model(&app, &key);
        app.set_model_token_draft("".into());
        app.set_status("模型已保存 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_delete_model(move |key| {
        let Some(app) = weak.upgrade() else { return };
        let key = key.trim().to_string();
        let models: Vec<_> = app
            .get_models()
            .iter()
            .filter(|entry| entry.key != key)
            .collect();
        if models.len() == app.get_models().row_count() {
            return;
        }
        store_models(&app, models);
        crate::entity_state::restore_model(&app, "");
        app.set_status("模型已删除 · 仅本次演示有效".into());
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
        store_models(&app, models);
        crate::entity_state::restore_model(&app, &key);
        app.set_status("默认模型已更新 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_move_model(move |from, delta| {
        let Some(app) = weak.upgrade() else { return };
        let Ok(from) = usize::try_from(from) else { return };
        let mut models: Vec<_> = app.get_models().iter().collect();
        if from >= models.len() || models.is_empty() {
            return;
        }
        let to = (from as i64 + i64::from(delta))
            .clamp(0, models.len() as i64 - 1) as usize;
        if to == from {
            return;
        }
        let selected_key = app.get_selected_model_key().to_string();
        let row = models.remove(from);
        models.insert(to, row);
        let selected = models
            .iter()
            .position(|entry| entry.key == selected_key)
            .map(|index| index as i32)
            .unwrap_or(-1);
        app.set_selected_model(selected);
        store_models(&app, models);
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
