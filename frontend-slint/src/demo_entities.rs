//! Entity actions in preview mode never persist configuration or credentials.
use crate::{MainWindow, ModelCard};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc};

thread_local! {
    /// The preview's model catalogue; the composer's picker renders exactly
    /// this list, mirroring the native shell's single-source projection.
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

/// Projects the catalogue and mirrors its size for the picker's "empty list"
/// versus "nothing configured" distinction.
fn project_models(app: &MainWindow) {
    DEMO_MODELS.with(|models| {
        let models = models.borrow();
        app.set_model_catalogue_count(models.len() as i32);
        app.set_models(ModelRc::new(VecModel::from(models.clone())));
    });
}

/// Replaces the preview catalogue; the model editor and the delete action both
/// go through here so the picker never sees a stale list.
fn store_models(app: &MainWindow, models: Vec<ModelCard>) {
    DEMO_MODELS.with(|slot| *slot.borrow_mut() = models);
    project_models(app);
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
    app.on_save_profile_avatar(|_, _| {});
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
    app.on_save_runtime(move || {
        if let Some(app) = weak.upgrade() {
            crate::help_browser::sync_language(&app);
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
    app.on_create_model(move || {
        let Some(app) = weak.upgrade() else { return };
        // Resume the parked or in-edit unsaved draft instead of stacking a
        // second row.
        if let Some(row_key) = crate::entity_state::active_model_draft_key(&app) {
            if let Some(index) = app
                .get_models()
                .iter()
                .position(|entry| entry.key == row_key.as_str())
            {
                app.set_selected_model(index as i32);
                if crate::entity_state::restore_model_draft(&app, index as i32, &row_key) {
                    app.set_status("已回到未保存的模型草稿 · 仅本次演示有效".into());
                } else {
                    app.set_status("当前正在编辑未保存的模型草稿 · 仅本次演示有效".into());
                }
                return;
            }
        }
        let mut models: Vec<_> = app.get_models().iter().collect();
        let mut key = "新模型".to_string();
        let mut suffix = 1usize;
        while models.iter().any(|entry| entry.key == key) {
            key = format!("新模型{suffix}");
            suffix += 1;
        }
        let entry = crate::entity_state::new_model_card(key);
        models.insert(0, entry.clone());
        store_models(&app, models);
        app.set_selected_model(0);
        crate::entity_state::apply_new_model_draft(&app, entry);
        app.set_status("已创建新的模型草稿 · 仅本次演示有效".into());
    });
    let weak = app.as_weak();
    app.on_discard_model_draft(move || {
        let Some(app) = weak.upgrade() else { return };
        let stashed_key = crate::entity_state::model_draft_row_key();
        if !app.get_model_creating() && stashed_key.is_none() {
            return;
        }
        let selected_key = usize::try_from(app.get_selected_model())
            .ok()
            .and_then(|index| app.get_models().row_data(index))
            .map(|row| row.key.to_string());
        let row_key = if app.get_model_creating() {
            selected_key.clone()
        } else {
            stashed_key.clone()
        };
        crate::entity_state::clear_model_draft();
        if let Some(row_key) = row_key {
            let models: Vec<_> = app
                .get_models()
                .iter()
                .filter(|entry| entry.key != row_key.as_str())
                .collect();
            store_models(&app, models);
        }
        if app.get_model_creating() {
            app.set_model_creating(false);
            crate::entity_state::restore_model(&app, "");
        }
    });
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
        // The saved row replaces the unsaved draft; drop the parked row so it
        // does not linger under its temporary key.
        if let Some(row_key) = crate::entity_state::model_draft_row_key() {
            if row_key != key {
                models.retain(|entry| entry.key != row_key.as_str());
            }
        }
        crate::entity_state::clear_model_draft();
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
        let Ok(from) = usize::try_from(from) else {
            return;
        };
        let mut models: Vec<_> = app.get_models().iter().collect();
        if from >= models.len() || models.is_empty() {
            return;
        }
        let to = (from as i64 + i64::from(delta)).clamp(0, models.len() as i64 - 1) as usize;
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

#[cfg(test)]
mod tests {
    use slint::Model;

    /// 回归：查看既有模型后再点「添加模型」，新卡片的编辑草稿必须是默认模板，
    /// 不得沿用上一个打开/查看过的模型的配置（提供方、工具调用方式、参数）。
    #[test]
    fn created_model_draft_does_not_inherit_the_viewed_model() {
        let _ui = crate::test_support::lock();
        crate::register_bundled_font().expect("注册内嵌字体");
        let app = crate::MainWindow::new().expect("创建主窗口");
        super::install(&app);
        // 打开既有模型查看：编辑器草稿投影为该模型的配置。
        let viewed = app
            .get_models()
            .iter()
            .position(|entry| entry.key == "demo-voice")
            .expect("演示目录缺少 asr 模型");
        app.invoke_select_model(viewed as i32);
        assert_eq!(app.get_model_type_draft().as_str(), "asr");
        assert_eq!(app.get_model_provider_draft().as_str(), "本地");

        app.invoke_create_model();

        assert!(app.get_model_creating(), "新建后应处于创建态");
        assert_eq!(app.get_model_key_draft().as_str(), "新模型");
        assert_eq!(app.get_model_provider_draft().as_str(), "openai_compatible");
        assert_eq!(app.get_model_type_draft().as_str(), "llm");
        assert_eq!(
            app.get_model_tool_call_mode_draft().as_str(),
            "function_call"
        );
        assert_eq!(app.get_model_name_draft().as_str(), "");
        assert_eq!(app.get_model_temperature_draft().as_str(), "0.7");
        assert_eq!(app.get_model_tts_speed_draft().as_str(), "1");
        assert_eq!(app.get_model_asr_temperature_draft().as_str(), "0");
        assert_eq!(app.get_model_image_steps_draft().as_str(), "30");
        assert_eq!(app.get_model_video_seconds_draft().as_str(), "4");
        let first = app.get_models().row_data(0).expect("新卡片缺席");
        assert_eq!(first.key.as_str(), "新模型");
        assert_eq!(first.provider.as_str(), "openai_compatible");
        assert_eq!(first.model_type.as_str(), "llm");
        assert_eq!(first.tool_call_mode.as_str(), "function_call");
    }

    /// 回归：未保存的新模型切换到其它条目后不丢弃——草稿行保留在列表里并
    /// 显示已编辑的值，切回该行恢复编辑草稿；再次「添加模型」恢复而非新建。
    #[test]
    fn unsaved_model_draft_survives_switching_entries() {
        let _ui = crate::test_support::lock();
        crate::register_bundled_font().expect("注册内嵌字体");
        let app = crate::MainWindow::new().expect("创建主窗口");
        super::install(&app);
        app.invoke_create_model();
        assert!(app.get_model_creating(), "新建后应处于创建态");
        app.set_model_provider_draft("deepseek".into());
        app.set_model_name_draft("draft-model".into());
        app.set_model_base_url_draft("https://example.invalid/v1".into());

        let existing = app
            .get_models()
            .iter()
            .position(|entry| entry.key == "demo-voice")
            .expect("演示目录缺少 asr 模型");
        app.invoke_select_model(existing as i32);
        assert!(!app.get_model_creating(), "切走后应退出创建态");
        let draft = app
            .get_models()
            .iter()
            .find(|entry| entry.key == "新模型")
            .expect("草稿行应保留在列表中");
        assert_eq!(draft.provider.as_str(), "deepseek");
        assert_eq!(draft.model.as_str(), "draft-model");
        assert_eq!(draft.base_url.as_str(), "https://example.invalid/v1");

        let draft_index = app
            .get_models()
            .iter()
            .position(|entry| entry.key == "新模型")
            .expect("草稿行缺席");
        app.invoke_select_model(draft_index as i32);
        assert!(app.get_model_creating(), "切回应恢复创建态");
        assert_eq!(app.get_model_provider_draft().as_str(), "deepseek");
        assert_eq!(app.get_model_name_draft().as_str(), "draft-model");
        assert_eq!(app.get_model_base_url_draft().as_str(), "https://example.invalid/v1");

        app.invoke_select_model(existing as i32);
        app.invoke_create_model();
        assert!(app.get_model_creating(), "再次添加应恢复草稿");
        assert_eq!(app.get_model_name_draft().as_str(), "draft-model");
        assert_eq!(
            app.get_models().iter().filter(|entry| entry.key == "新模型").count(),
            1,
            "不得出现第二张新模型卡片"
        );
    }
}
