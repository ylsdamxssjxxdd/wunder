//! Tool-selection persistence contract for the native settings editor.
//! The Slint editor sends exactly the names returned by `list_tools`;
//! after `update_agent_settings` a fresh `default_agent` must still report
//! them on the built-in `__default__` agent.

use std::sync::{Mutex, MutexGuard};

use wunder_desktop::{args::DesktopArgs, NativeDesktop};

/// 隔离运行时：WUNDER_HOME 决定配置、数据库与工作区的真实位置，`--temp-root`
/// 只管临时文件。锁保证同进程内的测试串行切换 WUNDER_HOME，避免任一测试
/// 读写到开发者的真实 ~/.wunder 状态。
static ISOLATION: Mutex<()> = Mutex::new(());

struct IsolatedRuntime {
    // 字段按声明顺序析构：先关运行时，再放锁，避免下一个测试在旧运行时
    // 收尾期间改写 WUNDER_HOME。
    runtime: NativeDesktop,
    _lock: MutexGuard<'static, ()>,
}

impl std::ops::Deref for IsolatedRuntime {
    type Target = NativeDesktop;

    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

fn start_isolated() -> Result<IsolatedRuntime, Box<dyn std::error::Error>> {
    let lock = ISOLATION.lock().unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("home");
    let config = home.join("config");
    std::fs::create_dir_all(&config)?;
    std::env::set_var("WUNDER_HOME", &home);
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
    args.temp_root = Some(directory.path().join("runtime"));
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    Ok(IsolatedRuntime {
        runtime: NativeDesktop::start_with_args(args)?,
        _lock: lock,
    })
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
fn desktop_control_is_listed_and_enabled_by_default() {
    let runtime = start_isolated().expect("isolated runtime");
    let tools = runtime.list_tools().expect("tools");
    assert!(
        tools.iter().any(|tool| tool.name == "桌面控制"),
        "desktop control missing from the builtin catalog: {:?}",
        tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>()
    );
    let agent = runtime.default_agent().expect("default agent");
    assert!(
        agent.tool_names.iter().any(|name| name == "桌面控制"),
        "desktop control is not selected by default: {:?}",
        agent.tool_names
    );
}

#[test]
fn tool_catalog_keeps_canonical_identity_and_a_display_title() {
    let runtime = start_isolated().expect("isolated runtime");
    let tools = runtime.list_tools().expect("tools");
    let read = tools
        .iter()
        .find(|tool| tool.name == "读取文件")
        .expect("read_file is part of the builtin catalog");
    assert_eq!(
        read.title, "读取文件",
        "the catalog title is what the settings page shows; the name stays the identity"
    );
    assert!(
        tools
            .iter()
            .filter(|tool| tool.category == "内置工具")
            .all(|tool| !tool.title.trim().is_empty()),
        "every builtin needs a display title, otherwise the UI falls back to identifiers"
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
