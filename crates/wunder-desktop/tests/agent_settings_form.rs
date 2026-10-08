//! Round-trip contract for the native settings page's `save-agent` action.
//!
//! The constants and field set mirror `frontend-slint/src/agent_editor.rs`:
//! the panel sends the twelve editor fields, and a fresh `default_agent` read
//! must report every one of them back. A façade that cannot persist one of
//! these fields leaves the settings form silently lossy.

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

/// Mirrors `AgentSettingsEdit` as the settings page builds it.
fn page_payload(tool_names: Vec<String>) -> wunder_desktop::AgentSettingsEdit {
    wunder_desktop::AgentSettingsEdit {
        name: "本地助手".into(),
        description: "整理工作目录并执行本地命令。".into(),
        system_prompt: "你是运行在本机的助手。".into(),
        model_name: "test-model".into(),
        icon_name: String::new(),
        icon_color: String::new(),
        icon: Some("{\"name\":\"robot\",\"color\":\"#3b82f6\"}".into()),
        tool_names,
        preset_questions: vec!["总结这个目录的结构".into(), "把最近的改动整理成清单".into()],
        approval_mode: "auto_edit".into(),
        preview_skill: true,
        silent: false,
        prefer_mother: true,
    }
}

#[test]
fn every_agent_form_field_survives_the_facade_round_trip() {
    let runtime = start_isolated().expect("isolated runtime");
    let names = runtime
        .list_tools()
        .expect("tools")
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    let saved = runtime
        .update_agent_settings(
            wunder_desktop::DEFAULT_AGENT_ID,
            page_payload(names.clone()),
        )
        .expect("the settings page payload saves");
    assert_eq!(saved.name, "本地助手");
    assert_eq!(saved.description, "整理工作目录并执行本地命令。");
    assert_eq!(saved.system_prompt, "你是运行在本机的助手。");
    assert_eq!(saved.model, "test-model");
    assert_eq!(
        saved.preset_questions,
        vec!["总结这个目录的结构", "把最近的改动整理成清单"]
    );
    assert_eq!(saved.approval_mode, "auto_edit");
    assert!(saved.preview_skill && saved.prefer_mother && !saved.silent);
    assert_eq!(saved.icon_config.name, "robot");
    assert_eq!(saved.icon_config.color, "#3b82f6");
    let missing = names
        .iter()
        .filter(|name| !saved.tool_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    assert!(missing.is_empty(), "tools lost after save: {missing:?}");

    // A second read proves the values are stored, not just echoed back.
    let reread = runtime.default_agent().expect("default agent");
    assert_eq!(reread.name, "本地助手");
    assert_eq!(reread.description, "整理工作目录并执行本地命令。");
    assert_eq!(reread.system_prompt, "你是运行在本机的助手。");
    assert_eq!(reread.model, "test-model");
    assert_eq!(reread.approval_mode, "auto_edit");
    assert!(reread.preview_skill && reread.prefer_mother && !reread.silent);
    assert_eq!(reread.preset_questions.len(), 2);
}

#[test]
fn an_empty_tool_selection_clears_the_set_instead_of_falling_back() {
    let runtime = start_isolated().expect("isolated runtime");
    let names = runtime
        .list_tools()
        .expect("tools")
        .into_iter()
        .map(|tool| tool.name)
        .collect::<Vec<_>>();
    runtime
        .update_agent_settings(wunder_desktop::DEFAULT_AGENT_ID, page_payload(names))
        .expect("first save");
    let cleared = runtime
        .update_agent_settings(wunder_desktop::DEFAULT_AGENT_ID, page_payload(Vec::new()))
        .expect("second save");
    assert!(
        cleared.tool_names.is_empty(),
        "clearing the switches must clear the stored selection: {:?}",
        cleared.tool_names
    );
}
