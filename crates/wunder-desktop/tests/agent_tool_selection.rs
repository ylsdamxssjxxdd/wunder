//! Tool-selection persistence contract for the native settings editor.
//! The Slint editor sends exactly the names returned by `list_tools`;
//! after `update_agent_settings` a fresh `default_agent` must still report
//! them on the built-in `__default__` agent.

use wunder_desktop::{args::DesktopArgs, NativeDesktop};

fn start_isolated() -> Result<NativeDesktop, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    Ok(NativeDesktop::start_with_args(args)?)
}

fn save_tools(runtime: &NativeDesktop, tool_names: Vec<String>) -> wunder_desktop::AgentRecord {
    runtime
        .update_agent_settings(
            "__default__",
            wunder_desktop::AgentSettingsEdit {
                name: "工具选择".into(),
                description: String::new(),
                system_prompt: String::new(),
                model_name: String::new(),
                icon_name: String::new(),
                icon_color: String::new(),
                icon: None,
                tool_names,
                preset_questions: Vec::new(),
                approval_mode: "suggest".into(),
                preview_skill: false,
                silent: false,
                prefer_mother: false,
            },
        )
        .expect("update settings")
}

#[test]
fn tool_selection_persists_for_default_agent() {
    let runtime = start_isolated().expect("isolated runtime");
    let tools = runtime.list_tools().expect("tools");
    assert!(!tools.is_empty(), "catalog must expose tools");
    let names = tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    let persisted = save_tools(&runtime, names.clone());
    assert_eq!(persisted.id, "__default__", "the single built-in agent");
    let missing = names
        .iter()
        .filter(|name| !persisted.tool_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "tools lost after save: {missing:?}\nlisted: {names:?}\npersisted: {:?}",
        persisted.tool_names
    );
}

#[test]
fn settings_updates_are_rejected_for_unknown_agents() {
    let runtime = start_isolated().expect("isolated runtime");
    let result = runtime.update_agent_settings(
        "agent_custom",
        wunder_desktop::AgentSettingsEdit {
            name: "不应该存在".into(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: String::new(),
            icon_name: String::new(),
            icon_color: String::new(),
            icon: None,
            tool_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: "suggest".into(),
            preview_skill: false,
            silent: false,
            prefer_mother: false,
        },
    );
    assert!(result.is_err(), "only the built-in agent is editable");
}
