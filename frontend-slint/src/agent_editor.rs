//! Agent editor unsaved-change tracking, form validation and save.
//!
//! The editor fields live in two-way-bound root properties, so edits project
//! straight into the drafts. A clean snapshot is captured whenever an agent is
//! loaded or saved; every field change recomputes the dirty flag against that
//! snapshot. Leaving the editor with pending edits mirrors the web settings
//! panel: a confirm dialog asks before the edits are discarded.
//!
//! Saving is validated here before it reaches the façade. The agent's prompt is
//! forwarded verbatim and never rewritten; the thread-level freeze is the
//! runtime's, so editing the record only shapes the threads that come after.

use crate::MainWindow;
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::sync::Arc;
use wunder_desktop::{AgentSettingsEdit, NativeDesktop};

/// Actions that would discard pending agent edits and therefore need a
/// confirmation while the editor is dirty.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum LeaveAction {
    SelectAgent(i32),
    RefreshAgents,
    ReturnChat,
}

thread_local! {
    static PENDING_LEAVE: RefCell<Option<LeaveAction>> = RefCell::new(None);
}

fn agent_form_snapshot(app: &MainWindow) -> String {
    serde_json::json!({
        "name": app.get_selected_agent_name().to_string(),
        "description": app.get_selected_agent_description().to_string(),
        "prompt": app.get_selected_agent_system_prompt().to_string(),
        "model": app.get_selected_agent_model().to_string(),
        "icon": app.get_selected_agent_icon_config().to_string(),
        "tools": app
            .get_selected_agent_tool_names()
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
        "questions": app
            .get_selected_agent_preset_questions()
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
        "approval": app.get_selected_agent_approval_mode().to_string(),
        "preview_skill": app.get_selected_agent_preview_skill(),
        "silent": app.get_selected_agent_silent(),
        "prefer_mother": app.get_selected_agent_prefer_mother(),
    })
    .to_string()
}

/// Capture the current editor values as the clean baseline and clear the flag.
/// Called after an agent is applied from backend data or saved successfully.
pub fn refresh_agent_snapshot(app: &MainWindow) {
    app.set_agent_form_snapshot(agent_form_snapshot(app).into());
    app.set_agent_dirty(false);
}

/// Recompute the dirty flag against the stored snapshot. Invoked from the
/// slint `changed` hooks of every editor-bound property and from in-place
/// model mutations (question edits) that bypass property reassignment.
pub fn refresh_agent_dirty(app: &MainWindow) {
    if app.get_selected_agent() < 0 {
        app.set_agent_dirty(false);
        return;
    }
    let current = agent_form_snapshot(app);
    app.set_agent_dirty(current != app.get_agent_form_snapshot().as_str());
}

/// Run the action immediately when the editor is clean; otherwise stash it and
/// raise the discard confirmation.
pub(crate) fn request_leave(app: &MainWindow, action: LeaveAction) {
    if !app.get_agent_dirty() {
        run_action(app, action);
        return;
    }
    PENDING_LEAVE.with(|slot| *slot.borrow_mut() = Some(action));
    app.set_agent_leave_confirm_open(true);
}

/// Editor values captured off the Slint properties, so the validation below is
/// a pure function the tests can drive without a running window.
pub(crate) struct AgentSaveDraft {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model: String,
    pub icon_config: String,
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub preview_skill: bool,
    pub silent: bool,
    pub prefer_mother: bool,
}

impl AgentSaveDraft {
    fn read(app: &MainWindow) -> Self {
        let strings = |values: slint::ModelRc<slint::SharedString>| -> Vec<String> {
            values.iter().map(|value| value.to_string()).collect()
        };
        Self {
            name: app.get_selected_agent_name().to_string(),
            description: app.get_selected_agent_description().to_string(),
            system_prompt: app.get_selected_agent_system_prompt().to_string(),
            model: app.get_selected_agent_model().to_string(),
            icon_config: app.get_selected_agent_icon_config().to_string(),
            tool_names: strings(app.get_selected_agent_tool_names()),
            preset_questions: strings(app.get_selected_agent_preset_questions()),
            approval_mode: app.get_selected_agent_approval_mode().to_string(),
            preview_skill: app.get_selected_agent_preview_skill(),
            silent: app.get_selected_agent_silent(),
            prefer_mother: app.get_selected_agent_prefer_mother(),
        }
    }
}

/// The runtime's approval-mode vocabulary. Anything else is normalised to the
/// default instead of being persisted as an unusable value.
fn normalize_approval_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "full_auto" | "full-auto" => "full_auto".to_string(),
        "auto_edit" | "auto-edit" => "auto_edit".to_string(),
        _ => "suggest".to_string(),
    }
}

/// Turn the editor drafts into a façade payload.
///
/// The rules mirror the runtime's own validation so a rejected save surfaces as
/// an inline message instead of a round-trip failure:
/// * a blank name is refused (it would erase the agent's name);
/// * the model must be one of the configured conversational models, and the
///   built-in agent must name the global default because the runtime pins it;
/// * the system prompt is forwarded verbatim and validated only for control
///   characters, never rewritten;
/// * at most 12 preset questions, matching the projection the list rendering
///   and the runtime normaliser both assume.
pub(crate) fn build_agent_save_edit(
    draft: &AgentSaveDraft,
    known_model_keys: &[String],
    default_model_key: &str,
) -> Result<AgentSettingsEdit, String> {
    let name = draft.name.trim();
    if name.is_empty() {
        return Err("请填写智能体名称".into());
    }
    if name.chars().count() > 64 || draft.description.chars().count() > 2048 {
        return Err("名称或问候语过长".into());
    }
    if draft
        .system_prompt
        .chars()
        .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
    {
        return Err("系统提示词包含无效字符".into());
    }
    let model = draft.model.trim();
    if !model.is_empty() {
        if !known_model_keys.iter().any(|key| key == model) {
            return Err("请选择「模型设置」中已配置的对话模型".into());
        }
        if !default_model_key.is_empty() && model != default_model_key {
            return Err("内置智能体跟随系统默认模型，请在模型设置中修改".into());
        }
    }
    let mut tool_names = draft
        .tool_names
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    tool_names.dedup();
    let mut preset_questions = draft
        .preset_questions
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .take(12)
        .collect::<Vec<_>>();
    preset_questions.dedup();
    Ok(AgentSettingsEdit {
        name: name.to_string(),
        description: draft.description.trim().to_string(),
        system_prompt: draft.system_prompt.clone(),
        model_name: model.to_string(),
        icon_name: String::new(),
        icon_color: String::new(),
        icon: (!draft.icon_config.trim().is_empty()).then(|| draft.icon_config.clone()),
        tool_names,
        preset_questions,
        approval_mode: normalize_approval_mode(&draft.approval_mode),
        preview_skill: draft.preview_skill,
        silent: draft.silent,
        prefer_mother: draft.prefer_mother,
    })
}

/// The global default conversational model, which the built-in agent follows.
fn default_model_key(app: &MainWindow) -> String {
    app.get_models()
        .iter()
        .find(|card| card.is_default && card.model_type == "llm")
        .map(|card| card.key.to_string())
        .unwrap_or_default()
}

/// The native `save-agent` handler. Without it the settings panel's save button
/// would fall through to nothing: the preview build binds this callback in
/// `demo_entities`, the native build previously bound it nowhere.
pub fn install_save_handler(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    app.on_save_agent_form(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() {
            return;
        }
        let draft = AgentSaveDraft::read(&app);
        let known = app
            .get_models()
            .iter()
            .map(|card| card.key.to_string())
            .collect::<Vec<_>>();
        let edit = match build_agent_save_edit(&draft, &known, &default_model_key(&app)) {
            Ok(edit) => edit,
            Err(error) => {
                app.set_status(format!("错误：{error}").into());
                app.set_dialog_title("无法保存智能体".into());
                app.set_dialog_text(error.into());
                app.set_dialog_open(true);
                return;
            }
        };
        // The runtime keys the built-in agent by a fixed id; the list only ever
        // holds that one record.
        let agent_id = app
            .get_agents()
            .row_data(0)
            .map(|agent| agent.id.to_string())
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| wunder_desktop::DEFAULT_AGENT_ID.to_string());
        app.set_saving(true);
        app.set_status("正在保存智能体设置…".into());
        let weak = weak.clone();
        let api = api.clone();
        crate::native_pages::run_background(move || {
            let result = api.update_agent_settings(&agent_id, edit);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_saving(false);
                match result {
                    Ok(agent) => {
                        let selected = app
                            .get_agents()
                            .row_data(0)
                            .map(|agent| agent.id.to_string())
                            .unwrap_or_default();
                        let card = crate::native_pages::agent_record_to_card(agent);
                        crate::native_pages::replace_agents(&app, vec![card]);
                        crate::entity_state::restore_agent(&app, &selected);
                        // The projection above re-enters `select-agent`, which
                        // rebases the snapshot; re-assert it so the editor is
                        // unambiguously clean after a successful save.
                        refresh_agent_snapshot(&app);
                        app.set_status("智能体设置已保存".into());
                    }
                    Err(error) => {
                        crate::native_pages::show_error(&app, format!("无法保存智能体设置：{error}"))
                    }
                }
            });
        });
    });
}

fn run_action(app: &MainWindow, action: LeaveAction) {    match action {
        LeaveAction::SelectAgent(index) => app.invoke_select_agent(index),
        LeaveAction::RefreshAgents => app.invoke_refresh_agents(),
        LeaveAction::ReturnChat => {
            let selected_id = usize::try_from(app.get_selected_agent())
                .ok()
                .and_then(|index| app.get_agents().row_data(index))
                .map(|agent| agent.id.to_string())
                .unwrap_or_default();
            if app.get_active_agent_id() != selected_id.as_str() {
                app.invoke_new_thread();
            } else {
                app.set_page(crate::DesktopPage::Messages);
            }
        }
    }
}

pub fn install(app: &MainWindow) {
    let weak = app.as_weak();
    app.on_refresh_agent_dirty(move || {
        if let Some(app) = weak.upgrade() {
            refresh_agent_dirty(&app);
        }
    });
    let weak = app.as_weak();
    app.on_agent_return_chat(move || {
        let Some(app) = weak.upgrade() else { return };
        request_leave(&app, LeaveAction::ReturnChat);
    });
    let weak = app.as_weak();
    app.on_agent_leave_confirmed(move || {
        let Some(app) = weak.upgrade() else { return };
        let action = PENDING_LEAVE.with(|slot| slot.borrow_mut().take());
        if let Some(action) = action {
            // The user accepted the discard: rebase the snapshot so the
            // follow-up action cannot re-trigger the guard.
            refresh_agent_snapshot(&app);
            run_action(&app, action);
        }
    });
    app.on_agent_leave_canceled(move || {
        PENDING_LEAVE.with(|slot| *slot.borrow_mut() = None);
    });
}

#[cfg(test)]
mod tests {
    use super::{build_agent_save_edit, normalize_approval_mode, AgentSaveDraft};

    fn draft() -> AgentSaveDraft {
        AgentSaveDraft {
            name: "内置智能体".into(),
            description: "本地单智能体".into(),
            system_prompt: "你是本地助手。".into(),
            model: "test-model".into(),
            icon_config: "{\"name\":\"robot\",\"color\":\"#3b82f6\"}".into(),
            tool_names: vec!["read_file".into(), "run_shell".into()],
            preset_questions: vec!["今天做什么？".into()],
            approval_mode: "suggest".into(),
            preview_skill: true,
            silent: false,
            prefer_mother: true,
        }
    }

    fn keys() -> Vec<String> {
        vec!["test-model".into(), "other-model".into()]
    }

    #[test]
    fn save_edit_carries_every_editor_field() {
        let edit = build_agent_save_edit(&draft(), &keys(), "test-model").expect("valid form");
        assert_eq!(edit.name, "内置智能体");
        assert_eq!(edit.description, "本地单智能体");
        assert_eq!(edit.model_name, "test-model");
        assert_eq!(edit.tool_names, vec!["read_file", "run_shell"]);
        assert_eq!(edit.preset_questions, vec!["今天做什么？"]);
        assert_eq!(edit.approval_mode, "suggest");
        assert!(edit.preview_skill && edit.prefer_mother && !edit.silent);
        assert_eq!(
            edit.icon.as_deref(),
            Some("{\"name\":\"robot\",\"color\":\"#3b82f6\"}")
        );
    }

    #[test]
    fn frozen_system_prompt_is_forwarded_verbatim() {
        let mut source = draft();
        source.system_prompt = "第一行\n第二行\t制表符".into();
        let edit = build_agent_save_edit(&source, &keys(), "test-model").expect("valid form");
        assert_eq!(edit.system_prompt, "第一行\n第二行\t制表符");
    }

    #[test]
    fn a_blank_name_is_refused_instead_of_erasing_the_agent() {
        let mut source = draft();
        source.name = "   ".into();
        assert!(build_agent_save_edit(&source, &keys(), "test-model").is_err());
    }

    #[test]
    fn an_unconfigured_model_is_refused() {
        let mut source = draft();
        source.model = "ghost-model".into();
        assert!(build_agent_save_edit(&source, &keys(), "test-model").is_err());
    }

    #[test]
    fn the_built_in_agent_follows_the_system_default_model() {
        let mut source = draft();
        source.model = "other-model".into();
        let refused = build_agent_save_edit(&source, &keys(), "test-model");
        assert!(
            refused.is_err(),
            "the runtime pins the built-in agent to the global default"
        );
        // The default is named explicitly rather than left empty, so an empty
        // global default (no conversational model configured) is the only case
        // that skips the check.
        let mut source = draft();
        source.model = "other-model".into();
        assert!(build_agent_save_edit(&source, &keys(), "").is_ok());
    }

    #[test]
    fn preset_questions_are_trimmed_deduplicated_and_capped() {
        let mut source = draft();
        source.preset_questions = (0..20).map(|index| format!(" 问题{index} ")).collect();
        let edit = build_agent_save_edit(&source, &keys(), "test-model").expect("valid form");
        assert_eq!(edit.preset_questions.len(), 12);
        assert_eq!(edit.preset_questions[0], "问题0");
        let mut source = draft();
        source.preset_questions = vec!["重复".into(), "重复".into(), "  ".into()];
        let edit = build_agent_save_edit(&source, &keys(), "test-model").expect("valid form");
        assert_eq!(edit.preset_questions, vec!["重复"]);
    }

    #[test]
    fn empty_tool_selection_is_a_valid_form() {
        let mut source = draft();
        source.tool_names = Vec::new();
        let edit = build_agent_save_edit(&source, &keys(), "test-model").expect("valid form");
        assert!(edit.tool_names.is_empty());
    }

    #[test]
    fn unknown_approval_modes_fall_back_to_the_runtime_default() {
        assert_eq!(normalize_approval_mode("full_auto"), "full_auto");
        assert_eq!(normalize_approval_mode("FULL-AUTO"), "full_auto");
        assert_eq!(normalize_approval_mode("auto_edit"), "auto_edit");
        assert_eq!(normalize_approval_mode(""), "suggest");
        assert_eq!(normalize_approval_mode("乱填"), "suggest");
    }

    #[test]
    fn control_characters_in_the_frozen_prompt_are_refused() {
        let mut source = draft();
        source.system_prompt = "坏\0提示词".into();
        assert!(build_agent_save_edit(&source, &keys(), "test-model").is_err());
    }
}
